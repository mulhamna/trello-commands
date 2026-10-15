//! Bulk update of 300 cards through the shared limiter, with a 429 mid-way.

mod common;

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use common::*;
use serde_json::json;
use trelloc_core::{
    ops::{self, BulkChanges},
    rate_limit::RateLimiter,
    Resolver, TrelloClient,
};
use wiremock::{matchers::method, Mock, MockServer, Request, Respond, ResponseTemplate};

/// Records arrival times; answers 429 for the 150th request only.
struct Recorder {
    calls: AtomicUsize,
    times: Arc<Mutex<Vec<Instant>>>,
}

impl Respond for Recorder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.times.lock().unwrap().push(Instant::now());
        if self.calls.fetch_add(1, Ordering::SeqCst) == 150 {
            return ResponseTemplate::new(429)
                .set_body_json(json!({ "error": "API_TOKEN_LIMIT_EXCEEDED" }));
        }
        let id = req.url.path().trim_start_matches("/cards/").to_string();
        ResponseTemplate::new(200).set_body_json(json!({ "id": id, "dueComplete": true }))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bulk_update_300_cards_paced_with_retry() {
    let server = MockServer::start().await;
    let cache = tempfile::tempdir().unwrap();
    let times = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("PUT"))
        .respond_with(Recorder {
            calls: AtomicUsize::new(0),
            times: times.clone(),
        })
        .mount(&server)
        .await;
    let cfg = cfg(&server, &cache);
    let window = Duration::from_millis(300);
    let c = TrelloClient::new(&cfg).unwrap().with_limiter(Arc::new(
        RateLimiter::with_limits(4, 90, window).with_backoff_base(Duration::from_millis(5)),
    ));
    let r = Resolver::new(&c, &cfg);
    let cards: Vec<String> = (0..300).map(|i| format!("6a{i:022x}")).collect();
    let done = AtomicUsize::new(0);
    let results = ops::bulk_update(
        &r,
        cards.clone(),
        &BulkChanges {
            due_complete: Some(true),
            ..Default::default()
        },
        || {
            done.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await
    .unwrap();
    let summary = ops::bulk_summary(&results);
    assert_eq!(summary["total"], 300);
    assert_eq!(summary["ok"], 300, "{summary}");
    assert_eq!(done.load(Ordering::SeqCst), 300);
    assert_eq!(
        results.iter().map(|r| r.card.clone()).collect::<Vec<_>>(),
        cards,
        "results keep input order"
    );
    // 300 cards + 1 retried 429.
    let times = times.lock().unwrap().clone();
    assert_eq!(times.len(), 301);
    // Unpaced, 300 requests would land in a few ms. Paced at 90 per 300 ms,
    // no half-window can see more than 90 (arrival jitter only shrinks gaps
    // by far less than 150 ms).
    let half = window / 2;
    let peak = times
        .iter()
        .map(|s| {
            times
                .iter()
                .filter(|t| **t >= *s && t.duration_since(*s) < half)
                .count()
        })
        .max()
        .unwrap();
    assert!(peak <= 90, "peak {peak} requests in {half:?}");
    let span = *times.iter().max().unwrap() - *times.iter().min().unwrap();
    assert!(
        span >= window * 3,
        "300 requests need > 3 windows, took {span:?}"
    );
    assert_no_credentials_in_urls(&server).await;
}

#[tokio::test]
async fn empty_changes_are_rejected() {
    let server = MockServer::start().await;
    let cache = tempfile::tempdir().unwrap();
    let cfg = cfg(&server, &cache);
    let c = TrelloClient::new(&cfg).unwrap();
    let r = Resolver::new(&c, &cfg);
    assert!(
        ops::bulk_update(&r, vec!["x".into()], &BulkChanges::default(), || {})
            .await
            .is_err()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}
