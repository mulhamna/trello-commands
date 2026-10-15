//! Human-input operations end to end against wiremock.

mod common;

use common::*;
use serde_json::{json, Value};
use trelloc_core::{
    ops::{self, CardInput, ChecklistInput, ItemUpdate, SearchInput, UpdateInput},
    Resolver, TrelloClient, TrelloError,
};
use wiremock::{
    matchers::{body_json, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

const B1: &str = "5f0000000000000000000001";
const B2: &str = "5f0000000000000000000002";

fn card_json(id: &str, board: &str, list: &str, n: u32) -> Value {
    json!({
        "id": id, "idShort": n, "name": format!("Card {n}"), "idBoard": board, "idList": list,
        "shortLink": format!("Cc{n:06}"), "shortUrl": format!("https://trello.com/c/Cc{n:06}"),
        "idMembers": ["me1"], "labels": [{ "id": "lb1", "name": "bug", "color": "red" }],
        "due": "2026-01-01T00:00:00.000Z", "dueComplete": false,
        "customFieldItems": [{ "idCustomField": "f1", "idValue": "o1" }]
    })
}

fn snapshot_for(id: &str, name: &str, short: &str) -> Value {
    let mut s = snapshot_json();
    s["id"] = json!(id);
    s["name"] = json!(name);
    s["shortLink"] = json!(short);
    s
}

async fn mount_snapshot(server: &MockServer, id: &str, snap: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/boards/{id}")))
        .and(query_param("lists", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(snap))
        .mount(server)
        .await;
}

async fn mount_directory(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/members/me/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/members/me/boards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            board_json(B1, "Roadmap", None),
            board_json(B2, "Ops", None),
        ])))
        .mount(server)
        .await;
}

struct Fixture {
    server: MockServer,
    _cache: tempfile::TempDir,
    cfg: trelloc_core::TrelloConfig,
}

impl Fixture {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let cache = tempfile::tempdir().unwrap();
        let mut cfg = cfg(&server, &cache);
        cfg.default_board = Some(B1.into());
        cfg.timezone = Some("UTC".into());
        mount_directory(&server).await;
        mount_snapshot(&server, B1, snapshot_for(B1, "Roadmap", "AbCd0001")).await;
        Self {
            server,
            _cache: cache,
            cfg,
        }
    }

    fn client(&self) -> TrelloClient {
        TrelloClient::new(&self.cfg).unwrap()
    }

    async fn requests(&self, m: &str, p: &str) -> Vec<wiremock::Request> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.method.as_str() == m && r.url.path() == p)
            .collect()
    }
}

#[tokio::test]
async fn scoped_search_is_one_batched_board_scan() {
    let f = Fixture::new().await;
    mount_snapshot(&f.server, B2, snapshot_for(B2, "Ops", "AbCd0002")).await;
    Mock::given(method("GET"))
        .and(path("/batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "200": [card_json("6a0000000000000000000001", B1, "l1", 1), card_json("6a0000000000000000000002", B1, "l2", 2)] },
            { "200": [card_json("6a0000000000000000000003", B2, "l1", 3)] }
        ])))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let res = ops::search_cards(
        &r,
        &SearchInput {
            boards: vec!["Roadmap".into(), "Ops".into()],
            mine: true,
            fields: vec!["Severity=High".into()],
            sort: Some("due".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(res.mode, "board_scan");
    assert_eq!(res.cards.len(), 3);
    let s = ops::card_summary(&res.cards[0], res.snapshot(B1), Some("UTC"));
    assert_eq!(s["ref"], "AbCd0001#1");
    assert_eq!(s["list"]["name"], "To Do");
    assert_eq!(s["members"][0]["username"], "me");
    assert!(f.requests("GET", "/search").await.is_empty());
    assert!(f.requests("GET", "/members/me").await.is_empty());
    assert_no_credentials_in_urls(&f.server).await;
}

#[tokio::test]
async fn list_scope_filters_by_list() {
    let f = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "200": [card_json("6a0000000000000000000001", B1, "l1", 1), card_json("6a0000000000000000000002", B1, "l2", 2)] }
        ])))
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let res = ops::search_cards(
        &r,
        &SearchInput {
            lists: vec!["Roadmap/Doing".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(res.cards.len(), 1);
    assert_eq!(res.cards[0].id_list, "l2");
}

#[tokio::test]
async fn query_uses_search_with_operators() {
    let f = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("query", "label:urgent -has:members is:open"))
        .and(query_param("modelTypes", "cards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "cards": [card_json("6a0000000000000000000001", B1, "l1", 1)]
        })))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let res = ops::search_cards(
        &r,
        &SearchInput {
            query: Some("label:urgent -has:members".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(res.mode, "search");
    assert_eq!(res.cards.len(), 1);
}

#[tokio::test]
async fn mine_unscoped_uses_member_cards_and_overdue_predicate() {
    let f = Fixture::new().await;
    let mut done = card_json("6a0000000000000000000002", B1, "l1", 2);
    done["dueComplete"] = json!(true);
    Mock::given(method("GET"))
        .and(path("/members/me/cards"))
        .and(query_param("filter", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            card_json("6a0000000000000000000001", B1, "l1", 1),
            done
        ])))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let res = ops::search_cards(
        &r,
        &SearchInput {
            mine: true,
            overdue: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(res.mode, "member_cards");
    assert_eq!(res.cards.len(), 1, "dueComplete cards are not overdue");
    assert!(f.requests("GET", "/members/me").await.is_empty());
}

#[tokio::test]
async fn field_filters_need_a_board() {
    let f = Fixture::new().await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let err = ops::search_cards(
        &r,
        &SearchInput {
            mine: true,
            fields: vec!["Severity=High".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, TrelloError::Invalid(_)));
}

#[tokio::test]
async fn create_sends_long_text_in_the_body_and_applies_fields_and_checklists() {
    let f = Fixture::new().await;
    let desc = "x".repeat(5000);
    Mock::given(method("POST"))
        .and(path("/cards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(
            "6a00000000000000000000c1",
            B1,
            "l2",
            7,
        )))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/cards/6a00000000000000000000c1/customField/f2/item"))
        .and(body_json(json!({ "value": { "number": "3" } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/cards/6a00000000000000000000c1/checklists"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "cl1", "name": "QA", "checkItems": [] })),
        )
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/checklists/cl1/checkItems"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "ci", "name": "x", "state": "incomplete" })),
        )
        .expect(2)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let out = ops::create_card(
        &r,
        &CardInput {
            name: "Fix login".into(),
            list: Some("Doing".into()),
            desc: Some(desc.clone()),
            members: vec!["alice".into()],
            labels: vec!["bug".into()],
            due: Some("2026-10-20".into()),
            pos: Some("top".into()),
            fields: [("Points".to_string(), "3".to_string())].into(),
            checklists: vec![ChecklistInput::parse("QA:one;two")],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let post = &f.requests("POST", "/cards").await[0];
    assert!(post.url.query().is_none(), "nothing in the URL");
    let body: Value = serde_json::from_slice(&post.body).unwrap();
    assert_eq!(body["idList"], "l2");
    assert_eq!(body["desc"].as_str().unwrap().len(), 5000);
    assert_eq!(body["idMembers"], "m1");
    assert_eq!(body["idLabels"], "lb1");
    assert_eq!(body["due"], "2026-10-20T00:00:00.000Z");
    assert_eq!(body["pos"], "top");
    assert_no_credentials_in_urls(&f.server).await;
}

#[tokio::test]
async fn add_label_only_uses_the_add_endpoint() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    Mock::given(method("GET"))
        .and(path(format!("/cards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B1, "l1", 1)))
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/cards/{id}/idLabels")))
        .and(body_json(json!({ "value": "lb2" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(["lb1", "lb2"])))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    ops::update_card(
        &r,
        id,
        &UpdateInput {
            labels_add: vec!["green".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(f.requests("PUT", &format!("/cards/{id}")).await.is_empty());
}

#[tokio::test]
async fn update_scalars_in_one_put_and_clear_due() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}")))
        .and(body_json(
            json!({ "name": "New", "due": null, "dueComplete": true }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B1, "l1", 1)))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    ops::update_card(
        &r,
        id,
        &UpdateInput {
            name: Some("New".into()),
            due: Some("none".into()),
            due_complete: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn cross_board_move_maps_labels_and_warns() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    let mut target = snapshot_for(B2, "Ops", "AbCd0002");
    target["labels"] = json!([{ "id": "tb1", "name": "BUG", "color": "orange" }]);
    target["members"] = json!([]);
    target["customFields"] = json!([]);
    mount_snapshot(&f.server, B2, target).await;
    let mut card = card_json(id, B1, "l1", 1);
    card["labels"] = json!([
        { "id": "lb1", "name": "bug", "color": "red" },
        { "id": "lb2", "name": "", "color": "green" }
    ]);
    Mock::given(method("GET"))
        .and(path(format!("/cards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card))
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}")))
        .and(body_json(
            json!({ "idList": "l2", "idBoard": B2, "idLabels": "tb1", "idMembers": "" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B2, "l2", 1)))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let out = ops::move_card(&r, id, "Ops/Doing", None).await.unwrap();
    let w = out.warnings.join("\n");
    assert!(w.contains("label 'green'"), "{w}");
    assert!(w.contains("member @me"), "{w}");
    assert!(w.contains("custom field 'Severity'"), "{w}");
    assert!(!w.contains("label 'bug'"), "bug maps to BUG by name: {w}");
}

#[tokio::test]
async fn same_board_move_has_no_warnings() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    Mock::given(method("GET"))
        .and(path(format!("/cards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B1, "l1", 1)))
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}")))
        .and(body_json(json!({ "idList": "l3", "pos": "bottom" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B1, "l3", 1)))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    let out = ops::move_card(&r, id, "Done", Some("bottom"))
        .await
        .unwrap();
    assert!(out.warnings.is_empty());
}

#[tokio::test]
async fn board_number_refs_resolve_via_card_numbers() {
    let f = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path(format!("/boards/{B1}/cards/all")))
        .and(query_param("fields", "id,idShort"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "c41", "idShort": 41 }, { "id": "c42", "idShort": 42 }
        ])))
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    assert_eq!(r.card("#42").await.unwrap(), "c42");
    assert_eq!(r.card("roadmap#41").await.unwrap(), "c41");
    assert_eq!(
        r.card("https://trello.com/c/AbCd1234/x").await.unwrap(),
        "AbCd1234"
    );
    assert!(matches!(r.card("#7").await, Err(TrelloError::NotFound(_))));
}

#[tokio::test]
async fn archive_and_delete() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}")))
        .and(body_json(json!({ "closed": true })))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B1, "l1", 1)))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/cards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "limits": {} })))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    ops::archive_card(&r, id, false).await.unwrap();
    assert_eq!(ops::delete_card(&r, id).await.unwrap(), id);
}

#[tokio::test]
async fn field_set_403_is_unavailable() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    Mock::given(method("GET"))
        .and(path(format!("/cards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(card_json(id, B1, "l1", 1)))
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}/customField/f1/item")))
        .respond_with(ResponseTemplate::new(403).set_body_string("unauthorized"))
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}/customField/f3/item")))
        .and(body_json(json!({ "value": { "checked": "true" } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}/customField/f4/item")))
        .and(body_json(json!({ "value": "" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);
    assert!(matches!(
        ops::set_card_field(&r, id, "Severity", "High").await,
        Err(TrelloError::Unavailable(_))
    ));
    ops::set_card_field(&r, id, "Reviewed", "true")
        .await
        .unwrap();
    ops::clear_card_field(&r, id, "Launch").await.unwrap();
}

#[tokio::test]
async fn comments_checklists_labels_attachments() {
    let f = Fixture::new().await;
    let id = "6a00000000000000000000c1";
    let long = "y".repeat(5000);
    Mock::given(method("POST"))
        .and(path(format!("/cards/{id}/actions/comments")))
        .and(body_json(json!({ "text": long })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1", "type": "commentCard", "date": "2026-10-14T00:00:00.000Z",
            "data": { "text": "hi" }, "memberCreator": { "id": "me1", "username": "me" }
        })))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/cards/{id}/actions")))
        .and(query_param("filter", "commentCard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "a1", "type": "commentCard", "date": "2026-10-14T00:00:00.000Z", "data": { "text": "hi" } }
        ])))
        .mount(&f.server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/cards/{id}/checklists")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "cl1", "name": "QA", "checkItems": [
                { "id": "i1", "name": "Write tests", "state": "incomplete", "idChecklist": "cl1" },
                { "id": "i2", "name": "Review", "state": "complete", "idChecklist": "cl1" }
            ] }
        ])))
        .mount(&f.server)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("/cards/{id}/checkItem/i1")))
        .and(body_json(json!({ "state": "complete" })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": "i1", "state": "complete" })),
        )
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/checklists/cl1/checkItems/i2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/labels"))
        .and(body_json(
            json!({ "name": "urgent", "color": "red_dark", "idBoard": B1 }),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "lb9", "name": "urgent", "color": "red_dark" })),
        )
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/cards/{id}/attachments")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "at1", "name": "x" })))
        .expect(2)
        .mount(&f.server)
        .await;
    let c = f.client();
    let r = Resolver::new(&c, &f.cfg);

    ops::comment_add(&r, id, &long).await.unwrap();
    let comments = ops::comment_list(&r, id, None).await.unwrap();
    assert_eq!(ops::comment_json(&comments[0], None)["text"], "hi");

    let item = ItemUpdate {
        checked: Some(true),
        ..Default::default()
    };
    ops::checklist_item_update(&r, id, "write", &item)
        .await
        .unwrap();
    assert_eq!(
        ops::checklist_item_delete(&r, id, "review")
            .await
            .unwrap()
            .id,
        "i2"
    );
    assert!(matches!(
        ops::checklist_item_update(&r, id, "e", &item).await,
        Err(TrelloError::Resolve { .. })
    ));

    ops::label_create(&r, None, "urgent", "red_dark")
        .await
        .unwrap();
    assert!(ops::label_color("chartreuse").is_err());

    ops::attachment_add(&r, id, "https://example.com/spec", Some("Spec"), false)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hello").unwrap();
    ops::attachment_add(&r, id, file.to_str().unwrap(), None, true)
        .await
        .unwrap();
    let uploads = f
        .requests("POST", &format!("/cards/{id}/attachments"))
        .await;
    let multipart = String::from_utf8_lossy(&uploads[1].body).to_string();
    assert!(multipart.contains("name=\"file\"") && multipart.contains("hello"));
    assert!(multipart.contains("name=\"setCover\""));
    assert!(ops::attachment_add(&r, id, "/no/such/file", None, false)
        .await
        .is_err());
    assert_no_credentials_in_urls(&f.server).await;
}
