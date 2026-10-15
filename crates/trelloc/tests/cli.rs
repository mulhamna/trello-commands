//! Runs the real `trelloc` binary against a wiremock server.

use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

struct Env {
    dir: tempfile::TempDir,
    base: Option<String>,
}

impl Env {
    /// With a server: a logged-in profile (fake key/token, cached member).
    fn new(base: Option<String>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        if base.is_some() {
            std::fs::create_dir_all(dir.path().join("config")).unwrap();
            std::fs::write(
                dir.path().join("config/config.toml"),
                r#"current_profile = "default"

[profiles.default]
api_key = "clikey"
token = "clitoken"
member_id = "me1"
username = "me"
full_name = "Me Myself"
default_board = "5f0000000000000000000001"
"#,
            )
            .unwrap();
        }
        Self { dir, base }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_trelloc"));
        c.args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("TRELLO_CONFIG_DIR", self.dir.path().join("config"))
            .env("TRELLO_CACHE_DIR", self.dir.path().join("cache"))
            .env("TRELLO_TIMEZONE", "UTC")
            .stdin(Stdio::null());
        if let Some(b) = &self.base {
            c.env("TRELLO_BASE_URL", b);
        }
        c
    }

    async fn run(&self, args: &[&str]) -> Output {
        let mut c = self.cmd(args);
        tokio::task::spawn_blocking(move || c.output().unwrap())
            .await
            .unwrap()
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

const BOARD: &str = "5f0000000000000000000001";

fn snapshot() -> Value {
    json!({
        "id": BOARD, "name": "Roadmap", "shortLink": "AbCd0001", "url": "https://trello.com/b/AbCd0001",
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
    Mock::given(method("GET"))
        .and(path(format!("/boards/{BOARD}/cards")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "c1", "idList": "l1" }, { "id": "c2", "idList": "l1" }, { "id": "c3", "idList": "l2" }
        ])))
        .mount(server)
        .await;
}

async fn members_me_calls(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == "/members/me")
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn version() {
    let o = Env::new(None).run(&["--version"]).await;
    assert!(o.status.success());
    assert!(stdout(&o).starts_with("trelloc "));
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_without_config() {
    let o = Env::new(None).run(&["auth", "status", "--json"]).await;
    assert!(o.status.success(), "{o:?}");
    let v: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["token"], Value::Null);
    assert_eq!(v["verified"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_credentials_exit_code_3() {
    let o = Env::new(None).run(&["board", "list"]).await;
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
}

#[tokio::test(flavor = "multi_thread")]
async fn whoami_uses_the_cache() {
    let server = MockServer::start().await;
    let env = Env::new(Some(server.uri()));
    let o = env.run(&["whoami", "--json"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let v: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["username"], "me");
    assert_eq!(members_me_calls(&server).await, 0);

    Mock::given(method("GET"))
        .and(path("/members/me"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "me1", "username": "me2", "fullName": "Renamed" })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let o = env.run(&["whoami", "--refresh", "--json"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let o = env.run(&["whoami", "--json"]).await;
    let v: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["username"], "me2", "refresh re-cached the member");
}

#[tokio::test(flavor = "multi_thread")]
async fn board_view_hits_the_snapshot_cache_the_second_time() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    let env = Env::new(Some(server.uri()));
    let o = env.run(&["board", "view", "--json"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let v: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["lists"][0]["cards"], 2);
    assert_eq!(v["lists"][1]["name"], "Doing");

    env.run(&["board", "view", BOARD]).await;
    let snapshot_calls = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == format!("/boards/{BOARD}"))
        .count();
    assert_eq!(snapshot_calls, 1);
    assert_eq!(members_me_calls(&server).await, 0);
    for r in server.received_requests().await.unwrap() {
        assert!(!r.url.as_str().contains("clitoken") && !r.url.as_str().contains("clikey"));
        assert!(r.url.query_pairs().all(|(k, _)| k != "key" && k != "token"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tree_batches_list_reads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/members/me/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "w1", "name": "acme", "displayName": "Acme" }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": BOARD, "name": "Roadmap", "shortLink": "AbCd0001", "idOrganization": "w1" }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "200": [{ "id": "l1", "name": "To Do" }, { "id": "l2", "name": "Doing" }] }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let o = Env::new(Some(server.uri())).run(&["tree"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(
        out.contains("Acme") && out.contains("Roadmap") && out.contains("Doing"),
        "{out}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_board_exit_code_4_with_suggestion() {
    let server = MockServer::start().await;
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
    let o = Env::new(Some(server.uri()))
        .run(&["board", "view", "Roadmp"])
        .await;
    assert_eq!(o.status.code(), Some(4));
    assert!(
        stderr(&o).contains("did you mean: Roadmap"),
        "{}",
        stderr(&o)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn api_write_refuses_without_yes() {
    let server = MockServer::start().await;
    let o = Env::new(Some(server.uri()))
        .run(&["api", "POST", "/cards", "--query", "name=x"])
        .await;
    assert!(!o.status.success());
    assert!(stderr(&o).contains("--yes"), "{}", stderr(&o));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn config_set_and_get() {
    let env = Env::new(None);
    let o = env
        .run(&["config", "set", "timezone", "Asia/Jakarta"])
        .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let o = env.run(&["config", "get", "timezone"]).await;
    assert_eq!(stdout(&o).trim(), "Asia/Jakarta");
    let o = env.run(&["config", "set", "timezone", "Mars/Base"]).await;
    assert!(!o.status.success());
}

#[tokio::test(flavor = "multi_thread")]
async fn card_list_defaults_to_the_default_board() {
    let server = MockServer::start().await;
    mount_board(&server).await;
    Mock::given(method("GET"))
        .and(path("/batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "200": [
                { "id": "6a0000000000000000000001", "idShort": 1, "name": "Ship it", "idBoard": BOARD, "idList": "l2",
                  "idMembers": ["me1"], "labels": [{ "id": "lb1", "name": "bug", "color": "red" }],
                  "due": "2026-10-20T00:00:00.000Z" }
            ] }
        ])))
        .mount(&server)
        .await;
    let o = Env::new(Some(server.uri())).run(&["card", "list"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    for want in ["AbCd0001#1", "Ship it", "Doing", "@me", "bug", "2026-10-20"] {
        assert!(out.contains(want), "{want} missing in:\n{out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn card_delete_refuses_without_yes() {
    let server = MockServer::start().await;
    let o = Env::new(Some(server.uri()))
        .run(&["card", "delete", "AbCd1234"])
        .await;
    assert!(!o.status.success());
    assert!(stderr(&o).contains("--yes"), "{}", stderr(&o));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn comment_add_reads_stdin_dash_text_from_arg() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cards/AbCd1234/actions/comments"))
        .and(wiremock::matchers::body_json(
            json!({ "text": "Done in staging" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "id": "a9", "type": "commentCard", "date": "2026-10-14T00:00:00.000Z" }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let o = Env::new(Some(server.uri()))
        .run(&["comment", "add", "AbCd1234", "Done in staging"])
        .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("a9"));
}
