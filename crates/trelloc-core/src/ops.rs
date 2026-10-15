//! Human-input operations shared by the CLI and the MCP server, so both
//! resolve names, parse dates and shape output the same way.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    client::{
        board_cards_url, field_clear_body, field_value_body, is_clear, Filter, Query,
        COMMENT_PAGE_SIZE, MAX_ACTION_PAGE, PAGE_SIZE,
    },
    error::{Result, TrelloError},
    filter::{parse_field_expr, CardFilter, Has, Plan, Range},
    model::{
        Action, Attachment, Board, BoardSnapshot, Card, CheckItem, Checklist, CustomField, Label,
        Member, Workspace,
    },
    resolve::{suggest, BoardId, Resolver},
    time::{is_none, parse_iso, parse_when, to_local_iso, when_iso},
};

// ── hierarchy ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BoardListInput {
    /// Workspace name or id; omit for every board the token can see.
    pub workspace: Option<String>,
    pub include_closed: bool,
    pub starred: bool,
}

pub async fn board_list(r: &Resolver<'_>, input: &BoardListInput) -> Result<Vec<Board>> {
    let filter = if input.include_closed {
        Filter::All
    } else {
        Filter::Open
    };
    let mut boards = match &input.workspace {
        Some(w) => {
            let ws = r.workspace(w).await?;
            r.client().workspace_boards(&ws.id, filter).await?
        }
        None => r.client().my_boards(filter).await?,
    };
    if input.starred {
        boards.retain(|b| b.starred);
    }
    Ok(boards)
}

/// `board view` / `trello_board_get`: the snapshot plus open-card counts
/// per list (one extra call).
pub async fn board_overview(r: &Resolver<'_>, board: &str) -> Result<Value> {
    let b = r.board(board).await?;
    let snap = r.snapshot(&b).await?;
    let ids: Vec<IdList> = r
        .client()
        .get(
            &format!("/boards/{}/cards", b.id),
            &Query::new().push("fields", "idList"),
        )
        .await?;
    Ok(overview_json(&snap, &ids))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdList {
    pub id_list: String,
}

fn overview_json(snap: &BoardSnapshot, cards: &[IdList]) -> Value {
    let mut lists = snap.lists.clone();
    lists.sort_by(|a, b| {
        a.pos
            .partial_cmp(&b.pos)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    json!({
        "id": snap.id,
        "name": snap.name,
        "url": snap.url,
        "short_link": snap.short_link,
        "workspace_id": snap.workspace_id,
        "lists": lists.iter().map(|l| json!({
            "id": l.id,
            "name": l.name,
            "cards": cards.iter().filter(|c| c.id_list == l.id).count(),
        })).collect::<Vec<_>>(),
        "labels": snap.labels.iter().map(|l| json!({
            "id": l.id, "name": l.name, "color": l.color,
        })).collect::<Vec<_>>(),
        "members": snap.members.iter().map(|m| json!({
            "id": m.id, "username": m.username, "full_name": m.full_name,
        })).collect::<Vec<_>>(),
        "custom_fields": snap.custom_fields.iter().map(|f| json!({
            "id": f.id,
            "name": f.name,
            "type": f.field_type,
            "options": f.options.iter().map(|o| o.value.text.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Members of a board, a workspace, or (neither given) the default board.
pub async fn member_list(
    r: &Resolver<'_>,
    board: Option<&str>,
    workspace: Option<&str>,
) -> Result<Vec<Member>> {
    if let Some(w) = workspace {
        let ws = r.workspace(w).await?;
        return r.client().workspace_members(&ws.id).await;
    }
    let b = r.board_or_default(board).await?;
    Ok(r.snapshot(&b).await?.members)
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeBoard {
    pub id: String,
    pub name: String,
    pub lists: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeWorkspace {
    pub id: Option<String>,
    pub name: String,
    pub boards: Vec<TreeBoard>,
}

/// Workspace → Board → List. Lists come from `/1/batch`, 10 boards per call.
pub async fn tree(r: &Resolver<'_>, workspace: Option<&str>) -> Result<Vec<TreeWorkspace>> {
    let dir = r.directory(false).await?;
    let only = match workspace {
        Some(w) => Some(r.workspace(w).await?.id),
        None => None,
    };
    let boards: Vec<&Board> = dir
        .boards
        .iter()
        .filter(|b| only.is_none() || b.workspace_id == only)
        .collect();
    let urls: Vec<String> = boards
        .iter()
        .map(|b| format!("/boards/{}/lists?fields=name&filter=open", b.id))
        .collect();
    let lists = r.client().batch(&urls).await?;
    let mut groups: Vec<TreeWorkspace> = Vec::new();
    let mut add = |ws: Option<&Workspace>, b: &Board, lists: Vec<String>| {
        let id = ws.map(|w| w.id.clone());
        let pos = match groups.iter().position(|g| g.id == id) {
            Some(i) => i,
            None => {
                groups.push(TreeWorkspace {
                    id: id.clone(),
                    name: ws.map_or_else(|| "Personal".into(), |w| w.display_name.clone()),
                    boards: vec![],
                });
                groups.len() - 1
            }
        };
        groups[pos].boards.push(TreeBoard {
            id: b.id.clone(),
            name: b.name.clone(),
            lists,
        });
    };
    for (b, l) in boards.iter().zip(lists) {
        let names = l
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|x| x["name"].as_str().map(str::to_string))
            .collect();
        let ws = dir
            .workspaces
            .iter()
            .find(|w| Some(&w.id) == b.workspace_id.as_ref());
        add(ws, b, names);
    }
    Ok(groups)
}

/// Board as shown in lists: id, name, shortLink, url, workspace.
pub fn board_summary(b: &Board) -> Value {
    json!({
        "id": b.id,
        "name": b.name,
        "short_link": b.short_link,
        "url": b.url,
        "workspace_id": b.workspace_id,
        "closed": b.closed,
        "starred": b.starred,
    })
}

/// Board id + name for outputs.
pub fn board_ref_json(b: &BoardId) -> Value {
    json!({ "id": b.id, "name": b.name })
}

// ── cards ───────────────────────────────────────────────────────────────

/// Card search in human terms. Every name may also be an id.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SearchInput {
    pub boards: Vec<String>,
    /// List names, ids or `Board/List`.
    pub lists: Vec<String>,
    /// `me`, usernames, full names or ids.
    pub members: Vec<String>,
    pub mine: bool,
    /// Label names or colors.
    pub labels: Vec<String>,
    pub due_before: Option<String>,
    pub due_after: Option<String>,
    pub overdue: bool,
    pub no_due: bool,
    pub due_complete: Option<bool>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub active_after: Option<String>,
    pub active_before: Option<String>,
    /// `attachments`, `description`, `members`, `cover`.
    pub has: Vec<String>,
    /// `Name<op>value` expressions, e.g. `Severity=High`, `Points>=3`.
    pub fields: Vec<String>,
    pub text: Option<String>,
    /// Raw Trello search syntax; forces search mode.
    pub query: Option<String>,
    pub include_archived: bool,
    pub sort: Option<String>,
    pub reverse: bool,
    pub limit: Option<usize>,
}

/// Matching cards plus the snapshots of their boards (for names).
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub cards: Vec<Card>,
    /// `board_scan`, `member_cards` or `search`.
    pub mode: &'static str,
    pub snapshots: BTreeMap<String, BoardSnapshot>,
}

impl SearchResult {
    pub fn snapshot(&self, board_id: &str) -> Option<&BoardSnapshot> {
        self.snapshots.get(board_id)
    }
}

fn when(input: &Option<String>, tz: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    input.as_deref().map(|s| parse_when(s, tz)).transpose()
}

/// Resolve names in `input` into a [`CardFilter`].
pub async fn build_filter(r: &Resolver<'_>, input: &SearchInput) -> Result<CardFilter> {
    let tz = r.client().timezone().map(str::to_string);
    let tz = tz.as_deref();
    let mut f = CardFilter::default();
    let mut boards: Vec<BoardId> = Vec::new();
    for b in &input.boards {
        let b = r.board(b).await?;
        if !boards.contains(&b) {
            boards.push(b);
        }
    }
    for l in &input.lists {
        let (b, list) = r
            .list_path(l, boards.first().map(|b| b.id.as_str()))
            .await?;
        f.lists.push(list.id);
        if !boards.contains(&b) {
            boards.push(b);
        }
    }
    // Plain listing with no scope at all: the default board, exact and cheap.
    let unscoped_listing = boards.is_empty()
        && !input.mine
        && input.query.is_none()
        && input.text.is_none()
        && input.members.is_empty()
        && input.labels.is_empty();
    if unscoped_listing {
        if let Some(d) = r.default_board() {
            boards.push(r.board(d).await?);
        }
    }
    if input.mine {
        let me = r.me()?;
        f.members.push(me.id.clone());
        f.member_names
            .push(me.username.clone().unwrap_or_else(|| "me".into()));
    }
    for m in &input.members {
        if m.trim().eq_ignore_ascii_case("me") {
            let me = r.me()?;
            f.members.push(me.id.clone());
            f.member_names
                .push(me.username.clone().unwrap_or_else(|| "me".into()));
            continue;
        }
        match boards.first() {
            Some(b) => {
                let member = r.member(b, m).await?;
                f.members.push(member.id);
                f.member_names.push(member.username);
            }
            // No board to resolve against: let search match the username.
            None => f
                .member_names
                .push(m.trim().trim_start_matches('@').to_string()),
        }
    }
    f.labels = input.labels.clone();
    f.due = Range {
        after: when(&input.due_after, tz)?,
        before: when(&input.due_before, tz)?,
    };
    f.created = Range {
        after: when(&input.created_after, tz)?,
        before: when(&input.created_before, tz)?,
    };
    f.activity = Range {
        after: when(&input.active_after, tz)?,
        before: when(&input.active_before, tz)?,
    };
    f.overdue = input.overdue;
    f.no_due = input.no_due;
    f.due_complete = input.due_complete;
    f.include_archived = input.include_archived;
    for h in &input.has {
        f.has.push(h.parse::<Has>()?);
    }
    for e in &input.fields {
        f.fields.push(parse_field_expr(e)?);
    }
    if !f.fields.is_empty() && boards.is_empty() {
        return Err(TrelloError::Invalid(
            "custom field filters need a board (--board); Trello search doesn't return field values"
                .into(),
        ));
    }
    f.text = input.text.clone().filter(|t| !t.trim().is_empty());
    f.raw_query = input.query.clone().filter(|q| !q.trim().is_empty());
    f.mine = input.mine;
    f.sort = input
        .sort
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_default();
    f.reverse = input.reverse;
    f.limit = input.limit;
    f.boards = boards.into_iter().map(|b| b.id).collect();
    Ok(f)
}

pub async fn search_cards(r: &Resolver<'_>, input: &SearchInput) -> Result<SearchResult> {
    let f = build_filter(r, input).await?;
    let c = r.client();
    let filter = if f.include_archived {
        Filter::All
    } else {
        Filter::Open
    };
    let (cards, mode) = match f.plan() {
        Plan::BoardScan { boards } => {
            let urls: Vec<String> = boards.iter().map(|b| board_cards_url(b, filter)).collect();
            let mut cards = Vec::new();
            for (board, page) in boards.iter().zip(c.batch(&urls).await?) {
                let page: Vec<Card> = serde_json::from_value(page?)?;
                cards.extend(c.page_rest(board, filter, page).await?);
            }
            (cards, "board_scan")
        }
        Plan::MemberCards => (c.my_cards(f.include_archived).await?, "member_cards"),
        Plan::Search { query } => {
            let limit = f.limit.unwrap_or(PAGE_SIZE).max(1);
            // Over-fetch: the exact predicate below may drop some results.
            let ids: Vec<String> = f.boards.clone();
            (
                c.search_cards(&query, &ids, (limit * 2).min(PAGE_SIZE))
                    .await?,
                "search",
            )
        }
    };
    let mut snapshots = BTreeMap::new();
    for board_id in cards
        .iter()
        .map(|c| c.id_board.clone())
        .collect::<BTreeSet<_>>()
    {
        let b = BoardId {
            id: board_id.clone(),
            name: board_id.clone(),
        };
        // ponytail: a board we can't snapshot just shows ids, not an error.
        if let Ok(s) = r.snapshot(&b).await {
            snapshots.insert(board_id, s);
        }
    }
    let now = Utc::now();
    let mut cards: Vec<Card> = cards
        .into_iter()
        .filter(|card| {
            let fields = snapshots
                .get(&card.id_board)
                .map(|s: &BoardSnapshot| s.custom_fields.as_slice())
                .unwrap_or_default();
            f.matches(card, now, |id| fields.iter().find(|d| d.id == id))
        })
        .collect();
    f.order(&mut cards);
    Ok(SearchResult {
        cards,
        mode,
        snapshots,
    })
}

/// `board#42`-style ref using the board's shortLink (resolvable anywhere).
pub fn card_ref(card: &Card, snap: Option<&BoardSnapshot>) -> String {
    let board = snap
        .map(|s| s.short_link.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(card.id_board.as_str());
    format!("{board}#{}", card.id_short)
}

fn iso_out(s: &Option<String>, tz: Option<&str>) -> Value {
    match s.as_deref().and_then(parse_iso) {
        Some(d) => json!(to_local_iso(d, tz)),
        None => Value::Null,
    }
}

/// Compact card shape shared by the CLI's `--json` and MCP outputs.
pub fn card_summary(card: &Card, snap: Option<&BoardSnapshot>, tz: Option<&str>) -> Value {
    json!({
        "id": card.id,
        "ref": card_ref(card, snap),
        "short_url": card.short_url,
        "name": card.name,
        "board": { "id": card.id_board, "name": snap.map(|s| s.name.clone()) },
        "list": { "id": card.id_list, "name": snap.and_then(|s| s.list_name(&card.id_list)) },
        "members": card.id_members.iter().map(|id| json!({
            "id": id,
            "username": snap.and_then(|s| s.member(id)).map(|m| m.username.clone()),
        })).collect::<Vec<_>>(),
        "labels": card.labels.iter().map(|l| json!({ "id": l.id, "name": l.name, "color": l.color })).collect::<Vec<_>>(),
        "due": iso_out(&card.due, tz),
        "due_complete": card.due_complete,
        "closed": card.closed,
    })
}

/// Summary plus description, dates, custom fields, checklists, attachments.
pub fn card_detail(card: &Card, snap: Option<&BoardSnapshot>, tz: Option<&str>) -> Value {
    let mut v = card_summary(card, snap, tz);
    let fields = snap.map(|s| s.custom_fields.as_slice()).unwrap_or_default();
    v["url"] = json!(card.url);
    v["short_link"] = json!(card.short_link);
    v["desc"] = json!(card.desc);
    v["start"] = iso_out(&card.start, tz);
    v["created"] = card
        .created_at()
        .map_or(Value::Null, |d| json!(to_local_iso(d, tz)));
    v["last_activity"] = iso_out(&card.date_last_activity, tz);
    v["subscribed"] = json!(card.subscribed);
    v["badges"] = json!(card.badges);
    v["custom_fields"] = json!(card
        .custom_field_items
        .iter()
        .map(|i| {
            let def = fields.iter().find(|f| f.id == i.id_custom_field);
            json!({
                "id": i.id_custom_field,
                "name": def.map(|d| d.name.clone()),
                "type": def.map(|d| d.field_type.clone()),
                "value": i.display_value(def),
            })
        })
        .collect::<Vec<_>>());
    v["checklists"] = json!(card
        .checklists
        .iter()
        .map(|c| {
            let (done, total) = c.progress();
            json!({
                "id": c.id, "name": c.name, "done": done, "total": total,
                "items": c.check_items.iter().map(|i| json!({
                    "id": i.id, "name": i.name, "done": i.done(), "due": iso_out(&i.due, tz),
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>());
    v["attachments"] = json!(card
        .attachments
        .iter()
        .map(|a| json!({ "id": a.id, "name": a.name, "url": a.url, "bytes": a.bytes }))
        .collect::<Vec<_>>());
    v
}

/// Fetch a card by any ref, with its board snapshot for names.
pub async fn get_card(
    r: &Resolver<'_>,
    card: &str,
    full: bool,
) -> Result<(Card, Option<BoardSnapshot>)> {
    let id = r.card(card).await?;
    let c = r.client().card(&id, full).await?;
    let snap = r
        .snapshot(&BoardId {
            id: c.id_board.clone(),
            name: c.id_board.clone(),
        })
        .await
        .ok();
    Ok((c, snap))
}

fn pos_value(pos: &str) -> Result<Value> {
    match pos.trim().to_ascii_lowercase().as_str() {
        "top" => Ok(json!("top")),
        "bottom" => Ok(json!("bottom")),
        other => other
            .parse::<f64>()
            .ok()
            .filter(|n| *n > 0.0)
            .map(|n| json!(n))
            .ok_or_else(|| {
                TrelloError::Invalid(format!(
                    "pos must be top, bottom or a positive number, got '{pos}'"
                ))
            }),
    }
}

/// `"none"` clears; anything else goes through the date parser.
fn date_value(input: &str, tz: Option<&str>) -> Result<Value> {
    if is_none(input) {
        Ok(Value::Null)
    } else {
        Ok(json!(when_iso(input, tz)?))
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CardInput {
    pub name: String,
    /// List name, id or `Board/List`; default: the profile's `default_list`.
    pub list: Option<String>,
    /// Board for a plain list name; default: the profile's `default_board`.
    pub board: Option<String>,
    /// Markdown.
    pub desc: Option<String>,
    /// `top`, `bottom` or a number.
    pub pos: Option<String>,
    pub members: Vec<String>,
    pub labels: Vec<String>,
    pub due: Option<String>,
    pub start: Option<String>,
    /// Custom field name → human value.
    pub fields: BTreeMap<String, String>,
    pub checklists: Vec<ChecklistInput>,
    /// Card to copy from (any card ref).
    pub copy_from: Option<String>,
    /// What to keep when copying: `all` or e.g. `checklists,labels,attachments`.
    pub keep: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ChecklistInput {
    pub name: String,
    pub items: Vec<String>,
}

impl ChecklistInput {
    /// `Name:item1;item2`.
    pub fn parse(s: &str) -> Self {
        let (name, items) = s.split_once(':').unwrap_or((s, ""));
        Self {
            name: name.trim().to_string(),
            items: items
                .split(';')
                .map(str::trim)
                .filter(|i| !i.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

/// Created card plus one line per follow-up step (fields, checklists).
#[derive(Debug, Clone)]
pub struct WriteResult {
    pub card: Card,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

pub async fn create_card(r: &Resolver<'_>, input: &CardInput) -> Result<WriteResult> {
    if input.name.trim().is_empty() && input.copy_from.is_none() {
        return Err(TrelloError::Invalid("card name is empty".into()));
    }
    let tz = r.client().timezone().map(str::to_string);
    let tz = tz.as_deref();
    let list_input = input
        .list
        .clone()
        .or_else(|| r.default_list().map(str::to_string))
        .ok_or_else(|| {
            TrelloError::Invalid(
                "no list given; pass --list (or set default_list with `trelloc config set default_list <list>`)"
                    .into(),
            )
        })?;
    let (board, list) = r.list_path(&list_input, input.board.as_deref()).await?;
    let mut body = json!({ "idList": list.id });
    if !input.name.trim().is_empty() {
        body["name"] = json!(input.name.trim());
    }
    if let Some(d) = &input.desc {
        body["desc"] = json!(d);
    }
    if let Some(p) = &input.pos {
        body["pos"] = pos_value(p)?;
    }
    if let Some(d) = &input.due {
        body["due"] = date_value(d, tz)?;
    }
    if let Some(d) = &input.start {
        body["start"] = date_value(d, tz)?;
    }
    let mut ids = Vec::new();
    for m in &input.members {
        ids.push(r.member(&board, m).await?.id);
    }
    if !ids.is_empty() {
        body["idMembers"] = json!(ids.join(","));
    }
    let mut ids = Vec::new();
    for l in &input.labels {
        ids.push(r.label(&board, l).await?.id);
    }
    if !ids.is_empty() {
        body["idLabels"] = json!(ids.join(","));
    }
    if let Some(src) = &input.copy_from {
        body["idCardSource"] = json!(r.card(src).await?);
        body["keepFromSource"] = json!(input.keep.as_deref().unwrap_or("all"));
    }
    let card = r.client().create_card(body).await?;
    let mut notes = Vec::new();
    let mut warnings = Vec::new();
    for (name, value) in &input.fields {
        match set_field(r, &board, &card.id, name, value).await {
            Ok(n) => notes.push(n),
            Err(e) => warnings.push(format!("field '{name}': {e}")),
        }
    }
    for cl in &input.checklists {
        match add_checklist(r, &card.id, &cl.name, &cl.items).await {
            Ok(c) => notes.push(format!(
                "checklist '{}' with {} items",
                c.name,
                c.check_items.len()
            )),
            Err(e) => warnings.push(format!("checklist '{}': {e}", cl.name)),
        }
    }
    Ok(WriteResult {
        card,
        notes,
        warnings,
    })
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct UpdateInput {
    pub name: Option<String>,
    pub desc: Option<String>,
    /// Date, or `none` to clear.
    pub due: Option<String>,
    pub start: Option<String>,
    pub due_complete: Option<bool>,
    pub members_add: Vec<String>,
    pub members_remove: Vec<String>,
    pub labels_add: Vec<String>,
    pub labels_remove: Vec<String>,
    /// Custom field name → human value.
    pub fields: BTreeMap<String, String>,
    pub fields_clear: Vec<String>,
    pub subscribed: Option<bool>,
    /// Cover color, or `none`.
    pub cover: Option<String>,
    pub closed: Option<bool>,
}

impl UpdateInput {
    fn needs_board(&self) -> bool {
        !(self.members_add.is_empty()
            && self.members_remove.is_empty()
            && self.labels_add.is_empty()
            && self.labels_remove.is_empty()
            && self.fields.is_empty()
            && self.fields_clear.is_empty())
    }
}

/// Add/remove members and labels through their own endpoints (never by
/// rewriting the whole set), fields through per-field PUTs, and everything
/// else in one `PUT /1/cards/{id}`.
pub async fn update_card(r: &Resolver<'_>, card: &str, input: &UpdateInput) -> Result<WriteResult> {
    let tz = r.client().timezone().map(str::to_string);
    let tz = tz.as_deref();
    let c = r.client();
    let id = r.card(card).await?;
    let mut body = serde_json::Map::new();
    if let Some(n) = &input.name {
        body.insert("name".into(), json!(n));
    }
    if let Some(d) = &input.desc {
        body.insert("desc".into(), json!(d));
    }
    if let Some(d) = &input.due {
        body.insert("due".into(), date_value(d, tz)?);
    }
    if let Some(d) = &input.start {
        body.insert("start".into(), date_value(d, tz)?);
    }
    if let Some(d) = input.due_complete {
        body.insert("dueComplete".into(), json!(d));
    }
    if let Some(s) = input.subscribed {
        body.insert("subscribed".into(), json!(s));
    }
    if let Some(cl) = input.closed {
        body.insert("closed".into(), json!(cl));
    }
    if let Some(color) = &input.cover {
        body.insert(
            "cover".into(),
            if is_none(color) {
                json!({ "color": null })
            } else {
                json!({ "color": color.trim().to_ascii_lowercase() })
            },
        );
    }
    let mut notes = Vec::new();
    let mut warnings = Vec::new();
    if input.needs_board() {
        let current = c.card(&id, false).await?;
        let board = BoardId {
            id: current.id_board.clone(),
            name: current.id_board.clone(),
        };
        for m in &input.members_add {
            let m = r.member(&board, m).await?;
            c.add_card_member(&id, &m.id).await?;
            notes.push(format!("added member @{}", m.username));
        }
        for m in &input.members_remove {
            let m = r.member(&board, m).await?;
            c.remove_card_member(&id, &m.id).await?;
            notes.push(format!("removed member @{}", m.username));
        }
        for l in &input.labels_add {
            let l = r.label(&board, l).await?;
            c.add_card_label(&id, &l.id).await?;
            notes.push(format!("added label {}", l.display()));
        }
        for l in &input.labels_remove {
            let l = r.label(&board, l).await?;
            c.remove_card_label(&id, &l.id).await?;
            notes.push(format!("removed label {}", l.display()));
        }
        for (name, value) in &input.fields {
            match set_field(r, &board, &id, name, value).await {
                Ok(n) => notes.push(n),
                Err(e) => warnings.push(format!("field '{name}': {e}")),
            }
        }
        for name in &input.fields_clear {
            match clear_field(r, &board, &id, name).await {
                Ok(n) => notes.push(n),
                Err(e) => warnings.push(format!("field '{name}': {e}")),
            }
        }
    }
    let card = if body.is_empty() {
        c.card(&id, false).await?
    } else {
        c.update_card(&id, Value::Object(body)).await?
    };
    Ok(WriteResult {
        card,
        notes,
        warnings,
    })
}

/// Move to a list (`List`, `Board/List`, alias). Across boards, labels are
/// mapped by name (or color) and anything missing on the target board is
/// reported in `warnings`.
pub async fn move_card(
    r: &Resolver<'_>,
    card: &str,
    to: &str,
    pos: Option<&str>,
) -> Result<WriteResult> {
    let c = r.client();
    let id = r.card(card).await?;
    let current = c.card(&id, false).await?;
    let (board, list) = r.list_path(to, Some(&current.id_board)).await?;
    let mut body = json!({ "idList": list.id });
    if let Some(p) = pos {
        body["pos"] = pos_value(p)?;
    }
    let mut warnings = Vec::new();
    if board.id != current.id_board {
        let source = r
            .snapshot(&BoardId {
                id: current.id_board.clone(),
                name: current.id_board.clone(),
            })
            .await?;
        let target = r.snapshot(&board).await?;
        let (labels, members, lost) = cross_board(&current, &source, &target);
        warnings = lost;
        body["idBoard"] = json!(board.id);
        body["idLabels"] = json!(labels.join(","));
        body["idMembers"] = json!(members.join(","));
    }
    let card = c.update_card(&id, body).await?;
    Ok(WriteResult {
        card,
        notes: vec![format!("moved to {}/{}", board.name, list.name)],
        warnings,
    })
}

/// Labels mapped onto the target board, members kept, and what gets lost.
fn cross_board(
    card: &Card,
    source: &BoardSnapshot,
    target: &BoardSnapshot,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut lost = Vec::new();
    for l in &card.labels {
        let hit = target.labels.iter().find(|t| {
            if l.name.is_empty() {
                t.name.is_empty() && t.color == l.color
            } else {
                t.name.eq_ignore_ascii_case(&l.name)
            }
        });
        match hit {
            Some(t) => labels.push(t.id.clone()),
            None => lost.push(format!(
                "label '{}' doesn't exist on board '{}' and will be dropped",
                l.display(),
                target.name
            )),
        }
    }
    let mut members = Vec::new();
    for m in &card.id_members {
        if target.member(m).is_some() {
            members.push(m.clone());
        } else {
            let name = source
                .member(m)
                .map(|x| format!("@{}", x.username))
                .unwrap_or_else(|| m.clone());
            lost.push(format!(
                "member {name} isn't on board '{}' and will be removed from the card",
                target.name
            ));
        }
    }
    for item in &card.custom_field_items {
        let def = source
            .custom_fields
            .iter()
            .find(|f| f.id == item.id_custom_field);
        let name = def
            .map(|d| d.name.clone())
            .unwrap_or_else(|| item.id_custom_field.clone());
        if !target
            .custom_fields
            .iter()
            .any(|f| f.name.eq_ignore_ascii_case(&name))
        {
            lost.push(format!(
                "custom field '{name}' doesn't exist on board '{}'; its value will be lost",
                target.name
            ));
        }
    }
    (labels, members, lost)
}

/// Copy a card into a list. `keep`: `all` (default) or e.g. `checklists,labels`.
pub async fn copy_card(
    r: &Resolver<'_>,
    card: &str,
    to: &str,
    name: Option<&str>,
    keep: Option<&str>,
) -> Result<WriteResult> {
    let id = r.card(card).await?;
    let (board, list) = r.list_path(to, None).await?;
    let mut body = json!({
        "idList": list.id,
        "idCardSource": id,
        "keepFromSource": keep.unwrap_or("all"),
        "pos": "bottom",
    });
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    let card = r.client().create_card(body).await?;
    Ok(WriteResult {
        card,
        notes: vec![format!("copied to {}/{}", board.name, list.name)],
        warnings: vec![],
    })
}

/// Archive (or unarchive): reversible, unlike delete.
pub async fn archive_card(r: &Resolver<'_>, card: &str, unarchive: bool) -> Result<Card> {
    let id = r.card(card).await?;
    r.client()
        .update_card(&id, json!({ "closed": !unarchive }))
        .await
}

/// Permanent delete.
pub async fn delete_card(r: &Resolver<'_>, card: &str) -> Result<String> {
    let id = r.card(card).await?;
    r.client().delete_card(&id).await?;
    Ok(id)
}

// ── collaboration ───────────────────────────────────────────────────────

async fn board_of_card(r: &Resolver<'_>, card_id: &str) -> Result<BoardId> {
    let c = r.client().card(card_id, false).await?;
    Ok(BoardId {
        id: c.id_board.clone(),
        name: c.id_board,
    })
}

/// Set one custom field from human input (option name, date, number,
/// true/false, text). `none` clears it.
pub async fn set_field(
    r: &Resolver<'_>,
    board: &BoardId,
    card_id: &str,
    field: &str,
    raw: &str,
) -> Result<String> {
    if is_clear(raw) {
        return clear_field(r, board, card_id, field).await;
    }
    let def = r.field(board, field).await?;
    let body = field_value_body(&def, raw, r.client().timezone())?;
    r.client().set_card_field(card_id, &def.id, body).await?;
    Ok(format!("set {} = {raw}", def.name))
}

pub async fn clear_field(
    r: &Resolver<'_>,
    board: &BoardId,
    card_id: &str,
    field: &str,
) -> Result<String> {
    let def = r.field(board, field).await?;
    r.client()
        .set_card_field(card_id, &def.id, field_clear_body(&def))
        .await?;
    Ok(format!("cleared {}", def.name))
}

/// `field set <card> <field> <value>` for any card ref.
pub async fn set_card_field(
    r: &Resolver<'_>,
    card: &str,
    field: &str,
    raw: &str,
) -> Result<String> {
    let id = r.card(card).await?;
    let board = board_of_card(r, &id).await?;
    set_field(r, &board, &id, field, raw).await
}

pub async fn clear_card_field(r: &Resolver<'_>, card: &str, field: &str) -> Result<String> {
    let id = r.card(card).await?;
    let board = board_of_card(r, &id).await?;
    clear_field(r, &board, &id, field).await
}

/// Field definitions of a board; `Unavailable` when the Power-Up is off
/// (Trello then returns an empty list).
pub async fn field_list(r: &Resolver<'_>, board: Option<&str>) -> Result<Vec<CustomField>> {
    let b = r.board_or_default(board).await?;
    let fields = r.client().board_fields(&b.id).await?;
    if fields.is_empty() {
        return Err(TrelloError::Unavailable(format!(
            "board '{}' has no custom fields (is the Custom Fields Power-Up enabled?)",
            b.name
        )));
    }
    Ok(fields)
}

// comments

/// Newest first. `limit` None = first page (50); pages up to Trello's max.
pub async fn comment_list(
    r: &Resolver<'_>,
    card: &str,
    limit: Option<usize>,
) -> Result<Vec<Action>> {
    let id = r.card(card).await?;
    let want = limit.unwrap_or(COMMENT_PAGE_SIZE);
    let mut out = Vec::new();
    for page in 0..=MAX_ACTION_PAGE {
        let batch = r.client().comments(&id, page).await?;
        let last = batch.len() < COMMENT_PAGE_SIZE;
        out.extend(batch);
        if last || out.len() >= want {
            break;
        }
    }
    out.truncate(want);
    Ok(out)
}

pub async fn comment_add(r: &Resolver<'_>, card: &str, text: &str) -> Result<Action> {
    if text.trim().is_empty() {
        return Err(TrelloError::Invalid("comment text is empty".into()));
    }
    let id = r.card(card).await?;
    r.client().add_comment(&id, text).await
}

pub async fn comment_edit(
    r: &Resolver<'_>,
    card: &str,
    comment_id: &str,
    text: &str,
) -> Result<Action> {
    if text.trim().is_empty() {
        return Err(TrelloError::Invalid("comment text is empty".into()));
    }
    let id = r.card(card).await?;
    r.client().edit_comment(&id, comment_id.trim(), text).await
}

pub fn comment_json(a: &Action, tz: Option<&str>) -> Value {
    json!({
        "id": a.id,
        "text": a.text(),
        "author": a.author(),
        "date": parse_iso(&a.date).map(|d| to_local_iso(d, tz)),
    })
}

// checklists

pub async fn add_checklist(
    r: &Resolver<'_>,
    card_id: &str,
    name: &str,
    items: &[String],
) -> Result<Checklist> {
    if name.trim().is_empty() {
        return Err(TrelloError::Invalid("checklist name is empty".into()));
    }
    let c = r.client();
    let mut cl = c.create_checklist(card_id, name.trim()).await?;
    for i in items.iter().filter(|i| !i.trim().is_empty()) {
        cl.check_items
            .push(c.add_check_item(&cl.id, i.trim(), None, None).await?);
    }
    Ok(cl)
}

pub async fn checklist_list(r: &Resolver<'_>, card: &str) -> Result<Vec<Checklist>> {
    let id = r.card(card).await?;
    r.client().card_checklists(&id).await
}

/// Checklist by id or (unique, case-insensitive) name on the card.
fn find_checklist<'a>(lists: &'a [Checklist], input: &str) -> Result<&'a Checklist> {
    let input = input.trim();
    if let Some(c) = lists.iter().find(|c| c.id == input) {
        return Ok(c);
    }
    let hits: Vec<&Checklist> = lists
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case(input))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => {
            let names: Vec<String> = lists.iter().map(|c| c.name.clone()).collect();
            Err(TrelloError::resolve(
                "checklist",
                input,
                suggest(input, &names),
            ))
        }
        _ => Err(TrelloError::Invalid(format!(
            "several checklists are named '{input}'; use the checklist id"
        ))),
    }
}

/// Item by id, exact text, or a unique substring (case-insensitive).
fn find_item<'a>(lists: &'a [Checklist], input: &str) -> Result<&'a CheckItem> {
    let input = input.trim();
    let items: Vec<&CheckItem> = lists.iter().flat_map(|c| &c.check_items).collect();
    if let Some(i) = items.iter().find(|i| i.id == input) {
        return Ok(i);
    }
    let lower = input.to_lowercase();
    let exact: Vec<&&CheckItem> = items
        .iter()
        .filter(|i| i.name.to_lowercase() == lower)
        .collect();
    let hits: Vec<&&CheckItem> = if exact.is_empty() {
        items
            .iter()
            .filter(|i| i.name.to_lowercase().contains(&lower))
            .collect()
    } else {
        exact
    };
    match hits.as_slice() {
        [one] => Ok(one),
        [] => {
            let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
            Err(TrelloError::resolve(
                "checklist item",
                input,
                suggest(input, &names),
            ))
        }
        many => Err(TrelloError::resolve(
            "checklist item",
            format!(
                "{input}' is ambiguous; matches: '{}",
                many.iter()
                    .map(|i| i.name.as_str())
                    .collect::<Vec<_>>()
                    .join("', '")
            ),
            vec![],
        )),
    }
}

pub async fn checklist_item_add(
    r: &Resolver<'_>,
    card: &str,
    checklist: &str,
    text: &str,
    due: Option<&str>,
    member: Option<&str>,
) -> Result<CheckItem> {
    let id = r.card(card).await?;
    let c = r.client();
    let lists = c.card_checklists(&id).await?;
    let cl = find_checklist(&lists, checklist)?;
    let due = due.map(|d| when_iso(d, c.timezone())).transpose()?;
    let member = match member {
        Some(m) => {
            let board = board_of_card(r, &id).await?;
            Some(r.member(&board, m).await?.id)
        }
        None => None,
    };
    c.add_check_item(&cl.id, text.trim(), due.as_deref(), member.as_deref())
        .await
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ItemUpdate {
    pub checked: Option<bool>,
    pub text: Option<String>,
    /// Date, or `none` to clear.
    pub due: Option<String>,
}

pub async fn checklist_item_update(
    r: &Resolver<'_>,
    card: &str,
    item: &str,
    update: &ItemUpdate,
) -> Result<CheckItem> {
    let id = r.card(card).await?;
    let c = r.client();
    let lists = c.card_checklists(&id).await?;
    let it = find_item(&lists, item)?;
    let mut body = json!({});
    if let Some(done) = update.checked {
        body["state"] = json!(if done { "complete" } else { "incomplete" });
    }
    if let Some(t) = &update.text {
        body["name"] = json!(t);
    }
    if let Some(d) = &update.due {
        body["due"] = date_value(d, c.timezone())?;
    }
    if body.as_object().is_some_and(|m| m.is_empty()) {
        return Err(TrelloError::Invalid(
            "nothing to update (checked, text or due)".into(),
        ));
    }
    c.update_check_item(&id, &it.id, body).await
}

pub async fn checklist_item_delete(r: &Resolver<'_>, card: &str, item: &str) -> Result<CheckItem> {
    let id = r.card(card).await?;
    let c = r.client();
    let lists = c.card_checklists(&id).await?;
    let it = find_item(&lists, item)?.clone();
    c.delete_check_item(&it.id_checklist, &it.id).await?;
    Ok(it)
}

pub async fn checklist_delete(r: &Resolver<'_>, card: &str, checklist: &str) -> Result<Checklist> {
    let id = r.card(card).await?;
    let c = r.client();
    let lists = c.card_checklists(&id).await?;
    let cl = find_checklist(&lists, checklist)?.clone();
    c.delete_checklist(&cl.id).await?;
    Ok(cl)
}

pub fn checklist_json(c: &Checklist) -> Value {
    let (done, total) = c.progress();
    json!({
        "id": c.id, "name": c.name, "done": done, "total": total,
        "items": c.check_items.iter().map(|i| json!({
            "id": i.id, "name": i.name, "done": i.done(), "due": i.due,
        })).collect::<Vec<_>>(),
    })
}

// labels

pub const LABEL_COLORS: [&str; 10] = [
    "green", "yellow", "orange", "red", "purple", "blue", "sky", "lime", "pink", "black",
];

/// A Trello label color (`red`, `red_dark`, `red_light`, …) or `none`.
pub fn label_color(input: &str) -> Result<Option<String>> {
    let c = input.trim().to_ascii_lowercase();
    if is_none(&c) {
        return Ok(None);
    }
    let base = c
        .strip_suffix("_dark")
        .or_else(|| c.strip_suffix("_light"))
        .unwrap_or(&c);
    if LABEL_COLORS.contains(&base) {
        Ok(Some(c))
    } else {
        Err(TrelloError::Invalid(format!(
            "unknown label color '{input}' ({}, with optional _dark/_light, or none)",
            LABEL_COLORS.join(", ")
        )))
    }
}

pub async fn label_list(r: &Resolver<'_>, board: Option<&str>) -> Result<Vec<Label>> {
    let b = r.board_or_default(board).await?;
    r.client().board_labels(&b.id).await
}

pub async fn label_create(
    r: &Resolver<'_>,
    board: Option<&str>,
    name: &str,
    color: &str,
) -> Result<Label> {
    let b = r.board_or_default(board).await?;
    let color = label_color(color)?;
    let l = r
        .client()
        .create_label(&b.id, name.trim(), color.as_deref())
        .await?;
    r.invalidate(&b.id);
    Ok(l)
}

pub async fn label_update(
    r: &Resolver<'_>,
    board: Option<&str>,
    label: &str,
    name: Option<&str>,
    color: Option<&str>,
) -> Result<Label> {
    let b = r.board_or_default(board).await?;
    let l = r.label(&b, label).await?;
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n.trim());
    }
    if let Some(c) = color {
        body["color"] = json!(label_color(c)?);
    }
    if body.as_object().is_some_and(|m| m.is_empty()) {
        return Err(TrelloError::Invalid(
            "nothing to update (name or color)".into(),
        ));
    }
    let out = r.client().update_label(&l.id, body).await?;
    r.invalidate(&b.id);
    Ok(out)
}

pub async fn label_delete(r: &Resolver<'_>, board: Option<&str>, label: &str) -> Result<Label> {
    let b = r.board_or_default(board).await?;
    let l = r.label(&b, label).await?;
    r.client().delete_label(&l.id).await?;
    r.invalidate(&b.id);
    Ok(l)
}

// attachments

pub async fn attachment_list(r: &Resolver<'_>, card: &str) -> Result<Vec<Attachment>> {
    let id = r.card(card).await?;
    r.client().attachments(&id).await
}

/// `target` is an http(s) URL or a local file path.
pub async fn attachment_add(
    r: &Resolver<'_>,
    card: &str,
    target: &str,
    name: Option<&str>,
    set_cover: bool,
) -> Result<Attachment> {
    let id = r.card(card).await?;
    let t = target.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        r.client().attach_url(&id, t, name, set_cover).await
    } else {
        r.client()
            .upload_attachment(&id, std::path::Path::new(t), name, set_cover)
            .await
    }
}

pub async fn attachment_delete(r: &Resolver<'_>, card: &str, attachment_id: &str) -> Result<()> {
    let id = r.card(card).await?;
    r.client()
        .delete_attachment(&id, attachment_id.trim())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrelloList;

    #[test]
    fn overview_counts_cards_per_list_in_position_order() {
        let snap = BoardSnapshot {
            id: "b".into(),
            name: "B".into(),
            lists: vec![
                TrelloList {
                    id: "l2".into(),
                    name: "Doing".into(),
                    pos: 2.0,
                    ..Default::default()
                },
                TrelloList {
                    id: "l1".into(),
                    name: "To Do".into(),
                    pos: 1.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let cards = vec![
            IdList {
                id_list: "l1".into(),
            },
            IdList {
                id_list: "l1".into(),
            },
            IdList {
                id_list: "l2".into(),
            },
        ];
        let v = overview_json(&snap, &cards);
        assert_eq!(v["lists"][0]["name"], "To Do");
        assert_eq!(v["lists"][0]["cards"], 2);
        assert_eq!(v["lists"][1]["cards"], 1);
    }
}
