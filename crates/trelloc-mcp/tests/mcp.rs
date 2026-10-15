//! In-process MCP tests over a duplex pipe. Trello is mocked with wiremock
//! via TRELLO_BASE_URL; nothing reaches the real API.

use rmcp::{
    model::CallToolRequestParams,
    service::{RoleClient, RunningService},
    ClientHandler, ServiceExt,
};
use serde_json::{json, Map, Value};
use serial_test::serial;
use tempfile::TempDir;
use trelloc_mcp::TrelloMcpServer;
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

#[derive(Default, Clone)]
struct TestClient;
impl ClientHandler for TestClient {}

const VARS: &[&str] = &[
    "TRELLO_API_KEY",
    "TRELLO_TOKEN",
    "TRELLO_PROFILE",
    "TRELLO_BASE_URL",
    "TRELLO_CONFIG_DIR",
    "TRELLO_CACHE_DIR",
    "TRELLO_TIMEZONE",
    "TRELLO_WORKSPACE",
    "TRELLO_DEFAULT_BOARD",
];

pub const BOARD: &str = "5f0000000000000000000001";

struct Env {
    _dir: TempDir,
}

impl Env {
    /// With a server: a logged-in profile file (fake credentials, cached member).
    fn new(base_url: Option<&str>) -> Self {
        let dir = TempDir::new().unwrap();
        for v in VARS {
            std::env::remove_var(v);
        }
        std::env::set_var("TRELLO_CONFIG_DIR", dir.path().join("config"));
        std::env::set_var("TRELLO_CACHE_DIR", dir.path().join("cache"));
        if let Some(url) = base_url {
            std::fs::create_dir_all(dir.path().join("config")).unwrap();
            std::fs::write(
                dir.path().join("config/config.toml"),
                format!(
                    "current_profile = \"default\"\n\n[profiles.default]\napi_key = \"mcpkey\"\ntoken = \"mcptoken\"\nmember_id = \"me1\"\nusername = \"me\"\nfull_name = \"Me Myself\"\ndefault_board = \"{BOARD}\"\ntimezone = \"UTC\"\n"
                ),
            )
            .unwrap();
            std::env::set_var("TRELLO_BASE_URL", url);
        }
        Self { _dir: dir }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for v in VARS {
            std::env::remove_var(v);
        }
    }
}

type Client = RunningService<RoleClient, TestClient>;

async fn connect() -> (Client, tokio::task::JoinHandle<()>) {
    let (server_io, client_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let s = TrelloMcpServer::new().serve(server_io).await.unwrap();
        let _ = s.waiting().await;
    });
    (TestClient.serve(client_io).await.unwrap(), server)
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

async fn call(client: &Client, name: &'static str, a: Value) -> Result<Value, String> {
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(args(a)))
        .await
        .map(|r| r.structured_content.unwrap_or(Value::Null)["result"].clone())
        .map_err(|e| e.to_string())
}

fn snapshot() -> Value {
    json!({
        "id": BOARD, "name": "Roadmap", "shortLink": "AbCd0001",
        "lists": [
            { "id": "l1", "name": "To Do", "pos": 1 },
            { "id": "l2", "name": "Doing", "pos": 2 }
        ],
        "labels": [{ "id": "lb1", "name": "bug", "color": "red" }],
        "members": [{ "id": "me1", "username": "me", "fullName": "Me Myself" }],
        "customFields": []
    })
}

async fn mount_board(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/boards/{BOARD}")))
        .and(query_param("lists", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(snapshot()))
        .mount(server)
        .await;
}

#[tokio::test]
#[serial]
async fn lists_tools_with_trello_prefix() {
    let _env = Env::new(None);
    let (client, _server) = connect().await;
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    for expected in [
        "trello_auth_status",
        "trello_whoami",
        "trello_profile_list",
        "trello_workspace_list",
        "trello_board_list",
        "trello_board_get",
        "trello_member_list",
        "trello_api_request",
    ] {
        assert!(names.contains(&expected.to_string()), "missing {expected}");
    }
    assert!(names.iter().all(|n| n.starts_with("trello_")));
}

#[tokio::test]
#[serial]
async fn auth_missing_without_credentials() {
    let _env = Env::new(None);
    let (client, _server) = connect().await;
    let err = call(&client, "trello_board_list", json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("auth_missing"), "{err}");
    let status = call(&client, "trello_auth_status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["credentials_present"], false);
}

#[tokio::test]
#[serial]
async fn whoami_is_cached_and_board_get_resolves_default_board() {
    let server = MockServer::start().await;
    let _env = Env::new(Some(&server.uri()));
    mount_board(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("/boards/{BOARD}/cards")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{ "id": "c1", "idList": "l2" }])),
        )
        .mount(&server)
        .await;
    let (client, _server) = connect().await;
    let me = call(&client, "trello_whoami", json!({})).await.unwrap();
    assert_eq!(me["username"], "me");
    let b = call(&client, "trello_board_get", json!({})).await.unwrap();
    assert_eq!(b["name"], "Roadmap");
    assert_eq!(b["lists"][1]["cards"], 1);
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.url.path() != "/members/me"));
    assert!(requests
        .iter()
        .all(|r| r.url.query_pairs().all(|(k, _)| k != "key" && k != "token")));
}

#[tokio::test]
#[serial]
async fn resolve_errors_carry_suggestions() {
    let server = MockServer::start().await;
    let _env = Env::new(Some(&server.uri()));
    Mock::given(method("GET"))
        .and(path("/members/me/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": BOARD, "name": "Roadmap", "shortLink": "AbCd0001" }
        ])))
        .mount(&server)
        .await;
    let (client, _server) = connect().await;
    let err = call(&client, "trello_board_get", json!({ "board": "Roadmp" }))
        .await
        .unwrap_err();
    assert!(
        err.contains("resolve_error") && err.contains("Roadmap"),
        "{err}"
    );
}

#[tokio::test]
#[serial]
async fn api_request_requires_confirm_for_writes() {
    let server = MockServer::start().await;
    let _env = Env::new(Some(&server.uri()));
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .and(query_param("fields", "name"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "name": "x" }])))
        .mount(&server)
        .await;
    let (client, _server) = connect().await;
    let err = call(
        &client,
        "trello_api_request",
        json!({ "method": "DELETE", "path": "/cards/abc" }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("unsafe_operation"), "{err}");
    let v = call(
        &client,
        "trello_api_request",
        json!({ "method": "GET", "path": "/members/me/boards", "query": { "fields": "name" } }),
    )
    .await
    .unwrap();
    assert_eq!(v[0]["name"], "x");
}

fn card(id: &str) -> Value {
    json!({
        "id": id, "idShort": 4, "name": "Fix login", "idBoard": BOARD, "idList": "l2",
        "shortUrl": "https://trello.com/c/Cc000004", "idMembers": ["me1"],
        "labels": [{ "id": "lb1", "name": "bug", "color": "red" }]
    })
}

#[tokio::test]
#[serial]
async fn card_delete_needs_confirm_but_archive_does_not() {
    let server = MockServer::start().await;
    let _env = Env::new(Some(&server.uri()));
    mount_board(&server).await;
    let id = "6a00000000000000000000c1";
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "closed": true, "id": id, "idBoard": BOARD })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/cards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let (client, _server) = connect().await;
    let err = call(&client, "trello_card_delete", json!({ "card": id }))
        .await
        .unwrap_err();
    assert!(err.contains("unsafe_operation"), "{err}");
    let v = call(&client, "trello_card_archive", json!({ "card": id }))
        .await
        .unwrap();
    assert_eq!(v["closed"], true);
    let v = call(
        &client,
        "trello_card_delete",
        json!({ "card": id, "confirm": true }),
    )
    .await
    .unwrap();
    assert_eq!(v["deleted"], true);
}

#[tokio::test]
#[serial]
async fn card_search_on_default_board_returns_refs() {
    let server = MockServer::start().await;
    let _env = Env::new(Some(&server.uri()));
    mount_board(&server).await;
    Mock::given(method("GET"))
        .and(path("/batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "200": [card("6a00000000000000000000c1")] }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let (client, _server) = connect().await;
    let v = call(
        &client,
        "trello_card_search",
        json!({ "labels": ["bug"], "boards": [BOARD] }),
    )
    .await
    .unwrap();
    assert_eq!(v["mode"], "board_scan");
    assert_eq!(v["cards"][0]["ref"], "AbCd0001#4");
    assert_eq!(v["cards"][0]["list"]["name"], "Doing");
    assert_eq!(v["cards"][0]["members"][0]["username"], "me");
}
