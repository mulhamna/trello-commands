//! Shared fixtures for wiremock tests. Fake credentials only.
#![allow(dead_code)]

use serde_json::{json, Value};
use trelloc_core::{config::CachedMember, TrelloClient, TrelloConfig};
use wiremock::{Match, MockServer, Request};

pub const KEY: &str = "testkey";
pub const TOKEN: &str = "testtoken";
pub const AUTH: &str = r#"OAuth oauth_consumer_key="testkey", oauth_token="testtoken""#;

pub fn cfg(server: &MockServer, cache: &tempfile::TempDir) -> TrelloConfig {
    let mut c = TrelloConfig::with_credentials(KEY, TOKEN, server.uri());
    c.cache_dir = cache.path().to_path_buf();
    c.member = Some(CachedMember {
        id: "me1".into(),
        username: Some("me".into()),
        full_name: Some("Me Myself".into()),
    });
    c
}

pub fn client(server: &MockServer) -> TrelloClient {
    let c = TrelloConfig::with_credentials(KEY, TOKEN, server.uri());
    TrelloClient::new(&c)
        .unwrap()
        .with_limiter(std::sync::Arc::new(
            trelloc_core::rate_limit::RateLimiter::default()
                .with_backoff_base(std::time::Duration::from_millis(1)),
        ))
}

pub fn board_json(id: &str, name: &str, workspace: Option<&str>) -> Value {
    let n = id.chars().rev().take(4).collect::<String>();
    let n: String = n.chars().rev().collect();
    json!({
        "id": id, "name": name, "shortLink": format!("AbCd{n}"),
        "url": format!("https://trello.com/b/AbCd{n}/x"), "idOrganization": workspace,
        "closed": false
    })
}

pub fn snapshot_json() -> Value {
    json!({
        "id": "b1", "name": "Roadmap", "shortLink": "AbCd0001", "idOrganization": "w1",
        "lists": [
            { "id": "l1", "name": "To Do", "pos": 1, "idBoard": "b1" },
            { "id": "l2", "name": "Doing", "pos": 2, "idBoard": "b1" },
            { "id": "l3", "name": "Done", "pos": 3, "idBoard": "b1" }
        ],
        "labels": [
            { "id": "lb1", "name": "bug", "color": "red", "idBoard": "b1" },
            { "id": "lb2", "name": "", "color": "green", "idBoard": "b1" }
        ],
        "members": [
            { "id": "m1", "username": "alice", "fullName": "Alice A" },
            { "id": "m2", "username": "bob", "fullName": "Bob Smith" },
            { "id": "me1", "username": "me", "fullName": "Me Myself" }
        ],
        "customFields": [
            { "id": "f1", "idModel": "b1", "name": "Severity", "type": "list",
              "options": [
                { "id": "o1", "value": { "text": "High" } },
                { "id": "o2", "value": { "text": "Low" } }
              ] },
            { "id": "f2", "idModel": "b1", "name": "Points", "type": "number" },
            { "id": "f3", "idModel": "b1", "name": "Reviewed", "type": "checkbox" },
            { "id": "f4", "idModel": "b1", "name": "Launch", "type": "date" },
            { "id": "f5", "idModel": "b1", "name": "Notes", "type": "text" }
        ]
    })
}

/// Fails the test if any request carried `key=` or `token=` in its URL.
pub async fn assert_no_credentials_in_urls(server: &MockServer) {
    for r in server.received_requests().await.unwrap_or_default() {
        let url = r.url.to_string();
        let leaks = r
            .url
            .query_pairs()
            .any(|(k, _)| k.eq_ignore_ascii_case("key") || k.eq_ignore_ascii_case("token"));
        assert!(!leaks, "credentials in URL: {url}");
        assert!(!url.contains(KEY) && !url.contains(TOKEN), "{url}");
    }
}

/// Exact `Authorization` header match (wiremock's `header()` splits on commas).
pub struct OAuthHeader;

impl Match for OAuthHeader {
    fn matches(&self, req: &Request) -> bool {
        req.headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            == Some(AUTH)
    }
}
