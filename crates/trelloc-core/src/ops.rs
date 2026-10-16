//! Human-input operations shared by the CLI and the MCP server, so both
//! resolve names, parse dates and shape output the same way.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    client::{
        board_cards_url, field_clear_body, field_value_body, is_clear, Filter, Query, ACTION_LIMIT,
        COMMENT_PAGE_SIZE, MAX_ACTION_PAGE, PAGE_SIZE,
    },
    error::{Result, TrelloError},
    filter::{parse_field_expr, CardFilter, Has, Plan, Range},
    model::{
        Action, Attachment, Board, BoardSnapshot, Card, CheckItem, Checklist, CustomField, Label,
        Member, TrelloList, Workspace,
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
    boards.retain(|b| r.board_is_active(&b.id));
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
    let (mut cards, mode) = match f.plan() {
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
    cards.retain(|card| r.board_is_active(&card.id_board));
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
    r.ensure_board_active(&board.id)?;
    if is_clear(raw) {
        return clear_field(r, board, card_id, field).await;
    }
    if r.board_access().is_restricted() {
        r.card(card_id).await?;
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
    r.ensure_board_active(&board.id)?;
    if r.board_access().is_restricted() {
        r.card(card_id).await?;
    }
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
    let card_id = if r.board_access().is_restricted() {
        r.card(card_id).await?
    } else {
        card_id.to_string()
    };
    let c = r.client();
    let mut cl = c.create_checklist(&card_id, name.trim()).await?;
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

// ── board & list management ─────────────────────────────────────────────

pub async fn board_create(
    r: &Resolver<'_>,
    name: &str,
    workspace: Option<&str>,
    from: Option<&str>,
    default_lists: bool,
) -> Result<Board> {
    if name.trim().is_empty() {
        return Err(TrelloError::Invalid("board name is empty".into()));
    }
    let ws = match workspace {
        Some(w) => Some(r.workspace(w).await?.id),
        None => None,
    };
    let src = match from {
        Some(b) => Some(r.board(b).await?.id),
        None => None,
    };
    let b = r
        .client()
        .create_board(name.trim(), ws.as_deref(), src.as_deref(), default_lists)
        .await?;
    r.invalidate_directory();
    Ok(b)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BoardUpdate {
    pub name: Option<String>,
    pub desc: Option<String>,
    /// true archives, false restores.
    pub closed: Option<bool>,
}

pub async fn board_update(r: &Resolver<'_>, board: &str, u: &BoardUpdate) -> Result<Board> {
    let b = r.board(board).await?;
    let mut body = serde_json::Map::new();
    if let Some(n) = &u.name {
        body.insert("name".into(), json!(n.trim()));
    }
    if let Some(d) = &u.desc {
        body.insert("desc".into(), json!(d));
    }
    if let Some(c) = u.closed {
        body.insert("closed".into(), json!(c));
    }
    if body.is_empty() {
        return Err(TrelloError::Invalid(
            "nothing to update (name, desc or closed)".into(),
        ));
    }
    let out = r.client().update_board(&b.id, Value::Object(body)).await?;
    r.invalidate_directory();
    r.invalidate(&b.id);
    Ok(out)
}

/// Permanent. Returns the deleted board.
pub async fn board_delete(r: &Resolver<'_>, board: &str) -> Result<BoardId> {
    let b = r.board(board).await?;
    r.client().delete_board(&b.id).await?;
    r.invalidate_directory();
    r.invalidate(&b.id);
    Ok(b)
}

pub async fn list_create(
    r: &Resolver<'_>,
    board: Option<&str>,
    name: &str,
    pos: Option<&str>,
) -> Result<TrelloList> {
    if name.trim().is_empty() {
        return Err(TrelloError::Invalid("list name is empty".into()));
    }
    let b = r.board_or_default(board).await?;
    let pos = pos.map(pos_value).transpose()?;
    let l = r.client().create_list(&b.id, name.trim(), pos).await?;
    r.invalidate(&b.id);
    Ok(l)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ListUpdate {
    pub name: Option<String>,
    /// `top`, `bottom` or a number.
    pub pos: Option<String>,
    /// true archives, false restores.
    pub closed: Option<bool>,
}

/// Lists are matched among open lists; restoring an archived list needs its id.
pub async fn list_update(
    r: &Resolver<'_>,
    board: Option<&str>,
    list: &str,
    u: &ListUpdate,
) -> Result<TrelloList> {
    let b = r.board_or_default(board).await?;
    let id = if crate::board_ref::is_id(list.trim()) {
        list.trim().to_string()
    } else {
        r.list(&b, list).await?.id
    };
    let mut body = serde_json::Map::new();
    if let Some(n) = &u.name {
        body.insert("name".into(), json!(n.trim()));
    }
    if let Some(p) = &u.pos {
        body.insert("pos".into(), pos_value(p)?);
    }
    if let Some(c) = u.closed {
        body.insert("closed".into(), json!(c));
    }
    if body.is_empty() {
        return Err(TrelloError::Invalid(
            "nothing to update (name, pos or closed)".into(),
        ));
    }
    let out = r.client().update_list(&id, Value::Object(body)).await?;
    r.invalidate(&b.id);
    Ok(out)
}

/// Move every card of `list` (on `board`) to `to` (`List` or `Board/List`).
pub async fn list_move_cards(
    r: &Resolver<'_>,
    board: Option<&str>,
    list: &str,
    to: &str,
) -> Result<(TrelloList, BoardId, TrelloList)> {
    let b = r.board_or_default(board).await?;
    let from = r.list(&b, list).await?;
    let (tb, tl) = r.list_path(to, Some(&b.id)).await?;
    r.client().move_all_cards(&from.id, &tb.id, &tl.id).await?;
    Ok((from, tb, tl))
}

pub async fn list_archive_cards(
    r: &Resolver<'_>,
    board: Option<&str>,
    list: &str,
) -> Result<TrelloList> {
    let b = r.board_or_default(board).await?;
    let l = r.list(&b, list).await?;
    r.client().archive_all_cards(&l.id).await?;
    Ok(l)
}

// ── bulk & batch ────────────────────────────────────────────────────────

/// Changes applied to every card in a bulk update.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BulkChanges {
    /// Move to this list (`List` or `Board/List`).
    pub list: Option<String>,
    pub labels_add: Vec<String>,
    pub labels_remove: Vec<String>,
    pub members_add: Vec<String>,
    pub members_remove: Vec<String>,
    /// Date or `none`.
    pub due: Option<String>,
    pub due_complete: Option<bool>,
    /// true archives every card.
    pub archive: Option<bool>,
    /// Add this comment to every card.
    pub comment: Option<String>,
}

impl BulkChanges {
    pub fn is_empty(&self) -> bool {
        self.list.is_none()
            && self.labels_add.is_empty()
            && self.labels_remove.is_empty()
            && self.members_add.is_empty()
            && self.members_remove.is_empty()
            && self.due.is_none()
            && self.due_complete.is_none()
            && self.archive.is_none()
            && self.comment.is_none()
    }

    /// One line per change, for dry runs and confirmations.
    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(l) = &self.list {
            out.push(format!("move to {l}"));
        }
        for (verb, v) in [
            ("add label", &self.labels_add),
            ("remove label", &self.labels_remove),
            ("add member", &self.members_add),
            ("remove member", &self.members_remove),
        ] {
            out.extend(v.iter().map(|x| format!("{verb} {x}")));
        }
        if let Some(d) = &self.due {
            out.push(format!("due {d}"));
        }
        if let Some(d) = self.due_complete {
            out.push(if d { "mark done" } else { "mark not done" }.into());
        }
        if self.archive == Some(true) {
            out.push("archive".into());
        } else if self.archive == Some(false) {
            out.push("unarchive".into());
        }
        if self.comment.is_some() {
            out.push("add comment".into());
        }
        out
    }
}

/// Outcome per card, in input order.
#[derive(Debug, Clone, Serialize)]
pub struct ItemResult {
    pub card: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl ItemResult {
    fn from(card: String, r: Result<Vec<String>>) -> Self {
        match r {
            Ok(warnings) => Self {
                card,
                ok: true,
                error: None,
                warnings,
            },
            Err(e) => Self {
                card,
                ok: false,
                error: Some(crate::error::redact(&e.to_string())),
                warnings: vec![],
            },
        }
    }
}

/// `{ total, ok, failed, results }`.
pub fn bulk_summary(results: &[ItemResult]) -> Value {
    let ok = results.iter().filter(|r| r.ok).count();
    json!({
        "total": results.len(),
        "ok": ok,
        "failed": results.len() - ok,
        "results": results,
    })
}

async fn apply_changes(r: &Resolver<'_>, card: &str, c: &BulkChanges) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let update = UpdateInput {
        due: c.due.clone(),
        due_complete: c.due_complete,
        members_add: c.members_add.clone(),
        members_remove: c.members_remove.clone(),
        labels_add: c.labels_add.clone(),
        labels_remove: c.labels_remove.clone(),
        closed: c.archive,
        ..Default::default()
    };
    let has_update = update.due.is_some()
        || update.due_complete.is_some()
        || update.closed.is_some()
        || update.needs_board();
    if has_update {
        warnings.extend(update_card(r, card, &update).await?.warnings);
    }
    if let Some(l) = &c.list {
        warnings.extend(move_card(r, card, l, None).await?.warnings);
    }
    if let Some(t) = &c.comment {
        comment_add(r, card, t).await?;
    }
    Ok(warnings)
}

/// Apply `changes` to each card (any refs) with bounded concurrency through
/// the shared limiter. `on_done` ticks once per card (progress bars).
pub async fn bulk_update(
    r: &Resolver<'_>,
    cards: Vec<String>,
    changes: &BulkChanges,
    on_done: impl Fn() + Send + Sync,
) -> Result<Vec<ItemResult>> {
    if changes.is_empty() {
        return Err(TrelloError::Invalid("no changes given".into()));
    }
    let limiter = r.client().limiter().clone();
    Ok(crate::bulk::run(
        &limiter,
        cards,
        |card| async move {
            let res = apply_changes(r, &card, changes).await;
            ItemResult::from(card, res)
        },
        on_done,
    )
    .await)
}

/// Create several cards in one list.
pub async fn bulk_create(
    r: &Resolver<'_>,
    list: Option<&str>,
    cards: Vec<CardInput>,
    on_done: impl Fn() + Send + Sync,
) -> Vec<(String, Result<WriteResult>)> {
    let limiter = r.client().limiter().clone();
    crate::bulk::run(
        &limiter,
        cards,
        |mut c| async move {
            if c.list.is_none() {
                c.list = list.map(str::to_string);
            }
            let name = c.name.clone();
            (name, create_card(r, &c).await)
        },
        on_done,
    )
    .await
}

/// One step of `trelloc card batch` / `trello_batch`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchOp {
    Create {
        #[serde(flatten)]
        card: CardInput,
    },
    Update {
        card: String,
        #[serde(flatten)]
        update: UpdateInput,
    },
    Move {
        card: String,
        list: String,
        #[serde(default)]
        pos: Option<String>,
    },
    Comment {
        card: String,
        text: String,
    },
    Archive {
        card: String,
        #[serde(default)]
        unarchive: bool,
    },
    Delete {
        card: String,
    },
}

impl BatchOp {
    pub fn describe(&self) -> String {
        match self {
            Self::Create { card } => format!(
                "create '{}' in {}",
                card.name,
                card.list.as_deref().unwrap_or("the default list")
            ),
            Self::Update { card, .. } => format!("update {card}"),
            Self::Move { card, list, .. } => format!("move {card} to {list}"),
            Self::Comment { card, .. } => format!("comment on {card}"),
            Self::Archive { card, unarchive } => {
                format!(
                    "{} {card}",
                    if *unarchive { "unarchive" } else { "archive" }
                )
            }
            Self::Delete { card } => format!("delete {card} (permanent)"),
        }
    }

    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Delete { .. })
    }
}

/// Run one batch step; the output is the card summary (or a deletion note).
pub async fn run_batch_op(r: &Resolver<'_>, op: &BatchOp) -> Result<Value> {
    let tz = r.client().timezone().map(str::to_string);
    let tz = tz.as_deref();
    let out = |w: WriteResult| {
        let mut v = card_summary(&w.card, None, tz);
        v["notes"] = json!(w.notes);
        v["warnings"] = json!(w.warnings);
        v
    };
    Ok(match op {
        BatchOp::Create { card } => out(create_card(r, card).await?),
        BatchOp::Update { card, update } => out(update_card(r, card, update).await?),
        BatchOp::Move { card, list, pos } => out(move_card(r, card, list, pos.as_deref()).await?),
        BatchOp::Comment { card, text } => {
            let a = comment_add(r, card, text).await?;
            comment_json(&a, tz)
        }
        BatchOp::Archive { card, unarchive } => {
            card_summary(&archive_card(r, card, *unarchive).await?, None, tz)
        }
        BatchOp::Delete { card } => json!({ "deleted": delete_card(r, card).await? }),
    })
}

// ── reports ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct Standup {
    pub member: String,
    pub since: String,
    /// Cards moved between lists since then: `{ card, from, to, date }`.
    pub moved: Vec<Value>,
    /// Cards marked done (due complete) since then.
    pub completed: Vec<Value>,
    /// Cards commented on since then.
    pub commented: Vec<Value>,
    /// Open cards I'm on (summaries).
    pub in_progress: Vec<Value>,
    pub due_soon: Vec<Value>,
    pub overdue: Vec<Value>,
}

/// My cards: what moved / got done / got comments since `since`, plus what's
/// open, due within two days, and overdue. `member` defaults to me.
pub async fn standup(
    r: &Resolver<'_>,
    since: &str,
    board: Option<&str>,
    member: Option<&str>,
) -> Result<Standup> {
    let tz = r.client().timezone().map(str::to_string);
    let since_dt = parse_when(since, tz.as_deref())?;
    let since_iso = crate::time::to_trello_iso(since_dt);
    let (board_id, member_id, member_name) = resolve_standup_scope(r, board, member).await?;
    let actions = standup_actions(r, board_id.as_ref(), &member_id, &since_iso).await?;
    let mut out =
        summarize_standup_actions(member_name, to_local_iso(since_dt, tz.as_deref()), &actions);
    let (in_progress, due_soon, overdue) =
        standup_card_summaries(r, board_id.as_ref(), member, &member_id, tz.as_deref()).await?;
    out.in_progress = in_progress;
    out.due_soon = due_soon;
    out.overdue = overdue;
    Ok(out)
}

async fn resolve_standup_scope(
    r: &Resolver<'_>,
    board: Option<&str>,
    member: Option<&str>,
) -> Result<(Option<BoardId>, String, String)> {
    let board_id = match board {
        Some(board) => Some(r.board(board).await?),
        None => None,
    };
    let (member_id, member_name) = match member {
        None | Some("me") => {
            let me = r.me()?;
            (
                me.id.clone(),
                me.username.clone().unwrap_or_else(|| me.id.clone()),
            )
        }
        Some(member) => {
            let board = match &board_id {
                Some(board) => board.clone(),
                None => r.board_or_default(None).await?,
            };
            let member = r.member(&board, member).await?;
            (member.id, member.username)
        }
    };
    Ok((board_id, member_id, member_name))
}

async fn standup_actions(
    r: &Resolver<'_>,
    board: Option<&BoardId>,
    member_id: &str,
    since: &str,
) -> Result<Vec<Action>> {
    let client = r.client();
    let actions = match board {
        Some(board) => {
            let mut actions = client
                .board_actions(
                    &board.id,
                    Some("updateCard,commentCard"),
                    Some(since),
                    ACTION_LIMIT,
                )
                .await?;
            actions.retain(|action| action.id_member_creator == member_id);
            actions
        }
        None => {
            client
                .get(
                    &format!("/members/{member_id}/actions"),
                    &Query::new()
                        .push("filter", "updateCard,commentCard")
                        .push("since", since)
                        .push("limit", ACTION_LIMIT),
                )
                .await?
        }
    };
    filter_active_actions(r, actions).await
}

async fn filter_active_actions(r: &Resolver<'_>, actions: Vec<Action>) -> Result<Vec<Action>> {
    let mut active = Vec::with_capacity(actions.len());
    for action in actions {
        if r.action_is_active(&action).await? {
            active.push(action);
        }
    }
    Ok(active)
}

fn summarize_standup_actions(member: String, since: String, actions: &[Action]) -> Standup {
    let mut out = Standup {
        member,
        since,
        ..Default::default()
    };
    let mut seen_done = BTreeSet::new();
    let mut seen_comment = BTreeSet::new();
    for action in actions {
        let card = json!({ "id": action.card_id(), "name": action.card_name() });
        let data = &action.data;
        if action.action_type == "updateCard" && data.get("listAfter").is_some() {
            out.moved.push(json!({
                "card": card,
                "from": data["listBefore"]["name"],
                "to": data["listAfter"]["name"],
                "date": action.date,
            }));
        } else if action.action_type == "updateCard"
            && data["card"]["dueComplete"] == json!(true)
            && data["old"].get("dueComplete").is_some()
        {
            if seen_done.insert(action.card_id().unwrap_or_default().to_string()) {
                out.completed.push(card);
            }
        } else if action.action_type == "commentCard"
            && seen_comment.insert(action.card_id().unwrap_or_default().to_string())
        {
            out.commented.push(card);
        }
    }
    out
}

async fn standup_card_summaries(
    r: &Resolver<'_>,
    board: Option<&BoardId>,
    member: Option<&str>,
    member_id: &str,
    tz: Option<&str>,
) -> Result<(Vec<Value>, Vec<Value>, Vec<Value>)> {
    let input = SearchInput {
        boards: board.iter().map(|board| board.id.clone()).collect(),
        members: if member.is_some_and(|member| member != "me") {
            vec![member_id.to_string()]
        } else {
            vec![]
        },
        mine: member.is_none_or(|member| member == "me"),
        ..Default::default()
    };
    let result = search_cards(r, &input).await?;
    let now = Utc::now();
    let soon = now + chrono::Duration::days(2);
    let mut in_progress = Vec::new();
    let mut due_soon = Vec::new();
    let mut overdue = Vec::new();
    for card in &result.cards {
        let summary = card_summary(card, result.snapshot(&card.id_board), tz);
        let due = card.due.as_deref().and_then(parse_iso);
        if !card.due_complete {
            match due {
                Some(date) if date < now => overdue.push(summary.clone()),
                Some(date) if date < soon => due_soon.push(summary.clone()),
                _ => {}
            }
            in_progress.push(summary);
        }
    }
    Ok((in_progress, due_soon, overdue))
}

/// Card counts for a board: per list, per member, overdue, no due date, done.
pub async fn board_report(r: &Resolver<'_>, board: Option<&str>) -> Result<Value> {
    let b = r.board_or_default(board).await?;
    let snap = r.snapshot(&b).await?;
    let cards = r.client().board_cards(&b.id, Filter::Open).await?;
    let now = Utc::now();
    let mut lists: Vec<_> = snap.lists.clone();
    lists.sort_by(|a, b| {
        a.pos
            .partial_cmp(&b.pos)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut per_member: BTreeMap<String, usize> = BTreeMap::new();
    for c in &cards {
        if c.id_members.is_empty() {
            *per_member.entry("(unassigned)".into()).or_default() += 1;
        }
        for m in &c.id_members {
            let name = snap
                .member(m)
                .map(|x| format!("@{}", x.username))
                .unwrap_or_else(|| m.clone());
            *per_member.entry(name).or_default() += 1;
        }
    }
    let overdue = cards
        .iter()
        .filter(|c| {
            !c.due_complete
                && c.due
                    .as_deref()
                    .and_then(parse_iso)
                    .is_some_and(|d| d < now)
        })
        .count();
    Ok(json!({
        "board": { "id": b.id, "name": snap.name },
        "total": cards.len(),
        "per_list": lists.iter().map(|l| json!({
            "list": l.name,
            "cards": cards.iter().filter(|c| c.id_list == l.id).count(),
        })).collect::<Vec<_>>(),
        "per_member": per_member,
        "overdue": overdue,
        "no_due": cards.iter().filter(|c| c.due.is_none()).count(),
        "done": cards.iter().filter(|c| c.due_complete).count(),
    }))
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ActivityInput {
    pub board: Option<String>,
    pub card: Option<String>,
    pub since: Option<String>,
    /// Only actions by this member.
    pub member: Option<String>,
    pub limit: Option<usize>,
}

/// Activity feed of a card, a board, or (neither given) my own actions.
pub async fn activity(r: &Resolver<'_>, input: &ActivityInput) -> Result<Vec<Value>> {
    let tz = r.client().timezone().map(str::to_string);
    let tz = tz.as_deref();
    let since = input
        .since
        .as_deref()
        .map(|s| when_iso(s, tz))
        .transpose()?;
    let limit = input.limit.unwrap_or(50).clamp(1, ACTION_LIMIT);
    let c = r.client();
    let (actions, board) = if let Some(card) = &input.card {
        let id = r.card(card).await?;
        (c.card_actions(&id, None, 0).await?, None)
    } else if let Some(b) = &input.board {
        let b = r.board(b).await?;
        (
            c.board_actions(&b.id, None, since.as_deref(), limit)
                .await?,
            Some(b),
        )
    } else {
        (c.my_actions(None, since.as_deref(), limit).await?, None)
    };
    let mut active_actions = Vec::with_capacity(actions.len());
    for action in actions {
        if r.action_is_active(&action).await? {
            active_actions.push(action);
        }
    }
    let mut actions = active_actions;
    if let Some(m) = &input.member {
        let id = if m.eq_ignore_ascii_case("me") {
            r.me()?.id.clone()
        } else {
            let b = match board {
                Some(b) => b,
                None => r.board_or_default(None).await?,
            };
            r.member(&b, m).await?.id
        };
        actions.retain(|a| a.id_member_creator == id);
    }
    if let Some(s) = since.as_deref().and_then(parse_iso) {
        actions.retain(|a| parse_iso(&a.date).is_none_or(|d| d >= s));
    }
    actions.truncate(limit);
    Ok(actions
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "type": a.action_type,
                "date": parse_iso(&a.date).map(|d| to_local_iso(d, tz)),
                "by": a.author(),
                "card": a.card_name(),
                "text": a.text(),
                "list_before": a.data.get("listBefore").and_then(|l| l.get("name")),
                "list_after": a.data.get("listAfter").and_then(|l| l.get("name")),
            })
        })
        .collect())
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
