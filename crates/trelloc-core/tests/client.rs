//! HTTP-level tests against a wiremock server. Never touches the real API.

mod common;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use common::*;
use serde_json::{json, Value};
use trelloc_core::{client::Filter, rate_limit::RateLimiter, Resolver, TrelloClient, TrelloError};
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, Request, Respond, ResponseTemplate,
};

fn rate_limited() -> ResponseTemplate {
    ResponseTemplate::new(429).set_body_json(json!({
        "error": "API_TOKEN_LIMIT_EXCEEDED",
        "message": "Rate limit exceeded"
    }))
}

/// Responds 429 for the first `n` calls, then 200 with `body`.
struct FailFirst {
    n: usize,
    calls: AtomicUsize,
    body: Value,
}

impl Respond for FailFirst {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.n {
            rate_limited()
        } else {
            ResponseTemplate::new(200).set_body_json(self.body.clone())
        }
    }
}

#[tokio::test]
async fn sends_oauth_header_and_never_credentials_in_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .and(OAuthHeader)
        .and(query_param("filter", "open"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([board_json("b1", "Roadmap", None)])),
        )
        .expect(1)
        .mount(&server)
        .await;
    let boards = client(&server).my_boards(Filter::Open).await.unwrap();
    assert_eq!(boards[0].name, "Roadmap");
    assert_no_credentials_in_urls(&server).await;
}

#[tokio::test]
async fn retries_after_429_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/boards/b1"))
        .respond_with(FailFirst {
            n: 1,
            calls: AtomicUsize::new(0),
            body: board_json("b1", "Roadmap", None),
        })
        .expect(2)
        .mount(&server)
        .await;
    let b = client(&server).board("b1").await.unwrap();
    assert_eq!(b.name, "Roadmap");
}

#[tokio::test]
async fn gives_up_after_four_retries_with_limit_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/boards/b1"))
        .respond_with(rate_limited())
        .expect(5)
        .mount(&server)
        .await;
    let err = client(&server).board("b1").await.unwrap_err();
    assert!(
        matches!(&err, TrelloError::RateLimit { limit } if limit == "API_TOKEN_LIMIT_EXCEEDED"),
        "{err:?}"
    );
}

#[tokio::test]
async fn plain_text_errors_map_to_typed_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/boards/unauth"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid token"))
        .mount(&server)
        .await;
    Mock::given(path("/boards/missing"))
        .respond_with(
            ResponseTemplate::new(404).set_body_string("The requested resource was not found."),
        )
        .mount(&server)
        .await;
    Mock::given(path("/boards/bad"))
        .respond_with(ResponseTemplate::new(400).set_body_string("invalid id"))
        .mount(&server)
        .await;
    let c = client(&server);
    assert!(matches!(c.board("unauth").await, Err(TrelloError::Auth(m)) if m == "invalid token"));
    assert!(matches!(
        c.board("missing").await,
        Err(TrelloError::NotFound(_))
    ));
    assert!(matches!(
        c.board("bad").await,
        Err(TrelloError::Api { status: 400, message, .. }) if message == "invalid id"
    ));
}

#[tokio::test]
async fn raw_rejects_credentials_and_merges_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .and(query_param("fields", "name"))
        .and(query_param("filter", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);
    let v = c
        .raw(
            "get",
            "/1/members/me/boards?fields=name",
            Some("filter=open"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(v, json!([]));
    assert!(matches!(
        c.raw("GET", "/members/me", Some("token=x"), None).await,
        Err(TrelloError::Invalid(_))
    ));
    assert!(matches!(
        c.raw("GET", "members/me", None, None).await,
        Err(TrelloError::Invalid(_))
    ));
}

#[tokio::test]
async fn batch_chunks_by_ten_and_maps_items() {
    let server = MockServer::start().await;
    let urls: Vec<String> = (0..12).map(|i| format!("/boards/b{i}")).collect();
    Mock::given(method("GET"))
        .and(path("/batch"))
        .and(query_param("urls", urls[..10].join(",")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                (0..10)
                    .map(|i| json!({ "200": { "id": format!("b{i}") } }))
                    .collect::<Vec<_>>(),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/batch"))
        .and(query_param("urls", urls[10..].join(",")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "200": { "id": "b10" } },
            { "404": "The requested resource was not found." }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let out = client(&server).batch(&urls).await.unwrap();
    assert_eq!(out.len(), 12);
    assert_eq!(out[10].as_ref().unwrap()["id"], "b10");
    assert!(matches!(out[11], Err(TrelloError::NotFound(_))));
}

#[tokio::test]
async fn shared_limiter_is_used_by_clones() {
    let server = MockServer::start().await;
    Mock::given(path("/boards/b1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(board_json("b1", "R", None)))
        .mount(&server)
        .await;
    let limiter = Arc::new(RateLimiter::default());
    let a = client(&server).with_limiter(limiter.clone());
    let b = a.clone();
    a.board("b1").await.unwrap();
    b.board("b1").await.unwrap();
    assert!(Arc::ptr_eq(a.limiter(), b.limiter()));
}

// ── resolver ────────────────────────────────────────────────────────────

async fn mount_directory(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/members/me/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "w1", "name": "acme", "displayName": "Acme" },
            { "id": "w2", "name": "side", "displayName": "Side" }
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            board_json("5f0000000000000000000001", "Roadmap", Some("w1")),
            board_json("5f0000000000000000000002", "Ops", Some("w1")),
            board_json("5f0000000000000000000003", "Ops", Some("w2")),
        ])))
        .mount(server)
        .await;
}

#[tokio::test]
async fn resolves_boards_by_id_link_url_alias_and_name() {
    let server = MockServer::start().await;
    let cache = tempfile::tempdir().unwrap();
    mount_directory(&server).await;
    Mock::given(method("GET"))
        .and(path("/boards/ZzZz9999"))
        .respond_with(ResponseTemplate::new(404).set_body_string("board not found"))
        .mount(&server)
        .await;
    let mut cfg = cfg(&server, &cache);
    cfg.aliases.insert("rm".into(), "Roadmap/Doing".into());
    let c = TrelloClient::new(&cfg).unwrap();
    let r = Resolver::new(&c, &cfg);

    let id = "5f0000000000000000000001";
    assert_eq!(r.board(id).await.unwrap().id, id);
    assert_eq!(r.board("AbCd0001").await.unwrap().name, "Roadmap");
    assert_eq!(
        r.board("https://trello.com/b/AbCd0001/roadmap")
            .await
            .unwrap()
            .id,
        id
    );
    assert_eq!(r.board("rm").await.unwrap().id, id);
    assert_eq!(r.board("roadmap").await.unwrap().id, id);

    match r.board("Ops").await.unwrap_err() {
        TrelloError::Resolve { input, .. } => {
            assert!(
                input.contains("Acme/Ops") && input.contains("Side/Ops"),
                "{input}"
            );
        }
        other => panic!("{other:?}"),
    }
    match r.board("Roadmp").await.unwrap_err() {
        TrelloError::Resolve { suggestions, .. } => assert_eq!(suggestions, vec!["Roadmap"]),
        other => panic!("{other:?}"),
    }
    // An unknown 8-char word falls back to name resolution.
    assert!(matches!(
        r.board("ZzZz9999").await,
        Err(TrelloError::Resolve { .. })
    ));
    // Narrowing to a workspace removes the ambiguity.
    let r = Resolver::new(&c, &cfg).with_workspace(Some("Side".into()));
    assert_eq!(r.board("Ops").await.unwrap().id, "5f0000000000000000000003");
}

#[tokio::test]
async fn snapshot_is_cached_on_disk() {
    let server = MockServer::start().await;
    let cache = tempfile::tempdir().unwrap();
    Mock::given(method("GET"))
        .and(path("/boards/b1"))
        .and(query_param("customFields", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(snapshot_json()))
        .expect(1)
        .mount(&server)
        .await;
    let cfg = cfg(&server, &cache);
    let c = TrelloClient::new(&cfg).unwrap();
    let board = trelloc_core::resolve::BoardId {
        id: "b1".into(),
        name: "Roadmap".into(),
    };
    {
        let r = Resolver::new(&c, &cfg);
        assert_eq!(r.list(&board, "doing").await.unwrap().id, "l2");
    }
    // A fresh resolver (next CLI invocation) reads the disk cache: zero HTTP.
    let r = Resolver::new(&c, &cfg);
    assert_eq!(r.list(&board, "1").await.unwrap().name, "To Do");
    assert_eq!(r.label(&board, "bug").await.unwrap().id, "lb1");
    assert_eq!(r.label(&board, "green").await.unwrap().id, "lb2");
    assert_eq!(r.member(&board, "@alice").await.unwrap().id, "m1");
    assert_eq!(r.member(&board, "Bob Smith").await.unwrap().id, "m2");
    assert_eq!(r.member(&board, "me").await.unwrap().id, "me1");
    assert_eq!(r.field(&board, "severity").await.unwrap().id, "f1");
}

#[tokio::test]
async fn snapshot_miss_refreshes_once() {
    let server = MockServer::start().await;
    let cache = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    struct Grow(Arc<AtomicUsize>);
    impl Respond for Grow {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let mut s = snapshot_json();
            if self.0.fetch_add(1, Ordering::SeqCst) > 0 {
                s["lists"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({ "id": "l9", "name": "Fresh", "pos": 99 }));
            }
            ResponseTemplate::new(200).set_body_json(s)
        }
    }
    Mock::given(path("/boards/b1"))
        .respond_with(Grow(calls.clone()))
        .mount(&server)
        .await;
    let cfg = cfg(&server, &cache);
    let c = TrelloClient::new(&cfg).unwrap();
    let board = trelloc_core::resolve::BoardId {
        id: "b1".into(),
        name: "Roadmap".into(),
    };
    Resolver::new(&c, &cfg).list(&board, "Doing").await.unwrap();
    let r = Resolver::new(&c, &cfg);
    assert_eq!(r.list(&board, "Fresh").await.unwrap().id, "l9");
    assert!(r.list(&board, "Nope").await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
