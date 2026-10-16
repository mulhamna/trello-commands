//! Tool implementations: validate → resolve names → call core → shape output.
//! Config is reloaded per call so `trelloc auth login` takes effect without
//! restarting the server; the rate limiter is shared per token.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, OnceLock},
};

use serde_json::{json, Value};
use trelloc_core::{
    config::{mask_secret, BoardAccess, BoardAccessFile, ConfigFile, TrelloConfig},
    ops::{self, BoardListInput},
    rate_limit::RateLimiter,
    Resolver, TrelloClient,
};

use crate::{
    error::{AppError, AppResult},
    models::*,
};

/// One limiter per token: Trello's budget is per token, so two accounts in
/// one server must not throttle each other.
fn limiter_for(token: &str) -> Arc<RateLimiter> {
    static LIMITERS: OnceLock<Mutex<HashMap<String, Arc<RateLimiter>>>> = OnceLock::new();
    LIMITERS
        .get_or_init(Default::default)
        .lock()
        .expect("limiter map lock")
        .entry(token.to_string())
        .or_insert_with(|| Arc::new(RateLimiter::default()))
        .clone()
}

pub struct Ctx {
    pub cfg: TrelloConfig,
    pub client: TrelloClient,
    board_access: BoardAccess,
    workspace: Option<String>,
}

impl Ctx {
    pub fn load(acc: &Account) -> AppResult<Self> {
        let cfg = load_config(acc.profile.as_deref())?;
        let (_, token) = cfg.credentials()?;
        let client = TrelloClient::new(&cfg)?.with_limiter(limiter_for(token));
        let board_access = BoardAccessFile::load()?.access(&cfg.profile);
        Ok(Self {
            cfg,
            client,
            board_access,
            workspace: acc.workspace.clone().filter(|w| !w.trim().is_empty()),
        })
    }

    pub fn resolver(&self) -> Resolver<'_> {
        Resolver::new(&self.client, &self.cfg)
            .with_workspace(self.workspace.clone())
            .with_board_access(self.board_access.clone())
    }

    pub fn tz(&self) -> Option<&str> {
        self.cfg.timezone.as_deref()
    }
}

fn load_config(profile: Option<&str>) -> AppResult<TrelloConfig> {
    if let Some(p) = profile {
        let file = ConfigFile::load()?;
        if !file.profiles.contains_key(p) {
            let names: Vec<&String> = file.profiles.keys().collect();
            return Err(AppError::config_error(format!(
                "unknown profile '{p}'; available: {names:?} (see trello_profile_list)"
            )));
        }
    }
    Ok(TrelloConfig::load_profile(profile)?)
}

pub fn require_confirm(confirm: Option<bool>) -> AppResult<()> {
    if confirm == Some(true) {
        Ok(())
    } else {
        Err(AppError::unsafe_operation(
            "this operation requires confirm=true",
        ))
    }
}

/// JSON value → the human string the core parsers expect.
pub fn value_to_input(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_to_input)
            .collect::<Vec<_>>()
            .join(","),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ── auth & meta ─────────────────────────────────────────────────────────

pub fn auth_status(a: Account) -> AppResult<Value> {
    let cfg = load_config(a.profile.as_deref())?;
    Ok(json!({
        "profile": cfg.profile,
        "member": cfg.member,
        "api_key": cfg.masked_key(),
        "token": cfg.masked_token(),
        "credentials_present": cfg.credentials().is_ok(),
        "default_board": cfg.default_board,
        "workspace": cfg.workspace,
        "config_path": cfg.config_path.display().to_string(),
        "timezone": cfg.timezone,
        "rate_limit": cfg.token.as_deref().map(|t| {
            let (token, key) = limiter_for(t).remaining();
            json!({ "token_remaining": token, "key_remaining": key })
        }),
    }))
}

/// Cached at login; never calls `/1/members/me`.
pub fn whoami(a: Account) -> AppResult<Value> {
    let cfg = load_config(a.profile.as_deref())?;
    let m = cfg.member()?;
    Ok(json!({ "id": m.id, "username": m.username, "full_name": m.full_name }))
}

pub fn profile_list() -> AppResult<Value> {
    let file = ConfigFile::load()?;
    let active = file.selected_profile_name();
    Ok(json!({
        "active": active,
        "profiles": file.profiles.iter().map(|(name, p)| json!({
            "name": name,
            "username": p.username,
            "default_board": p.default_board,
            "token": p.token.as_deref().map(mask_secret),
            "active": *name == active,
        })).collect::<Vec<_>>(),
        "hint": "Pass `profile` to any trello_* tool to run it as that account.",
    }))
}

pub async fn board_access(a: BoardAccessArgs) -> AppResult<Value> {
    let config = ConfigFile::load()?;
    let profile_name = a
        .account
        .profile
        .clone()
        .unwrap_or_else(|| config.selected_profile_name());
    if a.account.profile.is_some() && !config.profiles.contains_key(&profile_name) {
        return Err(AppError::config_error(format!(
            "unknown profile '{profile_name}'"
        )));
    }
    let mut access_file = BoardAccessFile::load()?;
    let mut access = access_file.access(&profile_name);

    let mut boards = None;
    let action = a.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "list" => {
            let mut cfg = TrelloConfig::from_file(&config, Some(&profile_name))?;
            cfg.workspace = a
                .account
                .workspace
                .clone()
                .filter(|workspace| !workspace.trim().is_empty())
                .or(cfg.workspace);
            let (_, token) = cfg.credentials()?;
            let client = TrelloClient::new(&cfg)?.with_limiter(limiter_for(token));
            let resolver = Resolver::new(&client, &cfg).with_board_access(BoardAccess::All);
            let directory = resolver.directory(false).await?;
            let workspace_id = match a.account.workspace.as_deref() {
                Some(workspace) if !workspace.trim().is_empty() => {
                    Some(resolver.workspace(workspace).await?.id)
                }
                _ => None,
            };
            boards = Some(json!(directory
                .boards
                .iter()
                .filter(|board| {
                    workspace_id
                        .as_deref()
                        .is_none_or(|id| board.workspace_id.as_deref() == Some(id))
                })
                .map(|board| json!({
                    "id": board.id,
                    "name": board.name,
                    "active": access.is_active(&board.id),
                }))
                .collect::<Vec<_>>()));
        }
        "all" => access.set_all(),
        "activate" | "deactivate" => {
            let board_input = a
                .board
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::validation("board is required for activate/deactivate"))?;
            let mut cfg = TrelloConfig::from_file(&config, Some(&profile_name))?;
            cfg.workspace = a
                .account
                .workspace
                .clone()
                .filter(|workspace| !workspace.trim().is_empty())
                .or(cfg.workspace);
            let (_, token) = cfg.credentials()?;
            let client = TrelloClient::new(&cfg)?.with_limiter(limiter_for(token));
            let resolver = Resolver::new(&client, &cfg).with_board_access(BoardAccess::All);
            let board = resolver.board(board_input).await?;
            if action == "activate" {
                access.activate(board.id);
            } else {
                access.deactivate(board.id);
            }
        }
        _ => {
            return Err(AppError::validation(
                "action must be list, all, activate or deactivate",
            ))
        }
    }
    if action != "list" {
        access_file.set(profile_name.clone(), access);
        access_file.save()?;
    }
    let access = access_file.access(&profile_name);
    Ok(json!({
        "profile": profile_name,
        "board_access": access,
        "configured": access.is_configured(),
        "restricted": access.is_restricted(),
        "boards": boards.unwrap_or(Value::Null),
    }))
}

// ── hierarchy ───────────────────────────────────────────────────────────

pub async fn workspace_list(a: Account) -> AppResult<Value> {
    let ctx = Ctx::load(&a)?;
    let ws = ctx.client.my_workspaces().await?;
    let current = ctx.cfg.workspace.clone();
    Ok(json!(ws
        .iter()
        .map(|w| json!({
            "id": w.id, "name": w.display_name, "short_name": w.name, "url": w.url,
            "default": current.as_deref().is_some_and(|c| c == w.id || c == w.name),
        }))
        .collect::<Vec<_>>()))
}

pub async fn board_list(a: BoardListArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let boards = ops::board_list(
        &ctx.resolver(),
        &BoardListInput {
            workspace: a.account.workspace.clone(),
            include_closed: a.include_closed.unwrap_or(false),
            starred: a.starred.unwrap_or(false),
        },
    )
    .await?;
    Ok(json!(boards
        .iter()
        .map(ops::board_summary)
        .collect::<Vec<_>>()))
}

pub async fn board_get(a: BoardArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let b = r.board_or_default(a.board.as_deref()).await?;
    Ok(ops::board_overview(&r, &b.id).await?)
}

pub async fn member_list(a: MemberListArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let members = ops::member_list(
        &ctx.resolver(),
        a.board.as_deref(),
        a.in_workspace.as_deref(),
    )
    .await?;
    Ok(json!(members
        .iter()
        .map(|m| json!({ "id": m.id, "username": m.username, "full_name": m.full_name }))
        .collect::<Vec<_>>()))
}

// ── escape hatch ────────────────────────────────────────────────────────

pub async fn api_request(a: ApiRequestArgs) -> AppResult<Value> {
    let method = a.method.trim().to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "DELETE") {
        return Err(AppError::validation(
            "method must be GET, POST, PUT or DELETE",
        ));
    }
    if method != "GET" {
        require_confirm(a.confirm)?;
    }
    let mut query = trelloc_core::Query::new();
    for (k, v) in a.query.unwrap_or_default() {
        if v.is_null() || v.is_array() || v.is_object() {
            return Err(AppError::validation(format!(
                "query parameter '{k}' must be a string, number or boolean"
            )));
        }
        query = query.push(&k, value_to_input(&v));
    }
    let ctx = Ctx::load(&a.account)?;
    if ctx.board_access.is_restricted() {
        return Err(AppError::unsafe_operation(
            "raw API requests are disabled while board access is restricted",
        ));
    }
    let encoded = query.encoded();
    Ok(ctx
        .client
        .raw(
            &method,
            &a.path,
            (!encoded.is_empty()).then_some(encoded.as_str()),
            a.body,
        )
        .await?)
}

// ── cards ───────────────────────────────────────────────────────────────

pub const DEFAULT_SEARCH_LIMIT: usize = 50;
pub const MAX_SEARCH_LIMIT: usize = 1000;

fn is_full(detail: &Option<String>, default_full: bool) -> bool {
    detail
        .as_deref()
        .map_or(default_full, |d| d.eq_ignore_ascii_case("full"))
}

fn strs(v: &Option<Vec<String>>) -> Vec<String> {
    v.clone().unwrap_or_default()
}

fn field_expr(f: &FieldFilterArg) -> String {
    let op = f.op.trim();
    match op.to_ascii_lowercase().as_str() {
        "empty" | "set" => format!("{} {op}", f.field),
        _ => format!(
            "{}{op}{}",
            f.field,
            f.value.as_ref().map(value_to_input).unwrap_or_default()
        ),
    }
}

pub fn search_input(a: &CardSearchArgs) -> ops::SearchInput {
    ops::SearchInput {
        boards: strs(&a.boards),
        lists: strs(&a.lists),
        members: strs(&a.members),
        labels: strs(&a.labels),
        due_before: a.due_before.clone(),
        due_after: a.due_after.clone(),
        overdue: a.overdue.unwrap_or(false),
        no_due: a.no_due.unwrap_or(false),
        due_complete: a.due_complete,
        created_after: a.created_after.clone(),
        active_after: a.active_after.clone(),
        has: strs(&a.has),
        fields: a.fields.iter().flatten().map(field_expr).collect(),
        text: a.text.clone(),
        query: a.query.clone(),
        include_archived: a.include_archived.unwrap_or(false),
        sort: a.sort.clone(),
        limit: Some(
            a.limit
                .unwrap_or(DEFAULT_SEARCH_LIMIT)
                .clamp(1, MAX_SEARCH_LIMIT),
        ),
        ..Default::default()
    }
}

pub async fn card_search(a: CardSearchArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let res = ops::search_cards(&r, &search_input(&a)).await?;
    let full = is_full(&a.detail, false);
    let tz = ctx.tz();
    Ok(json!({
        "mode": res.mode,
        "note": (res.mode == "search").then_some("Trello search results may lag a few seconds behind recent changes; scope by board for exact results."),
        "count": res.cards.len(),
        "cards": res.cards.iter().map(|c| {
            let snap = res.snapshot(&c.id_board);
            if full { ops::card_detail(c, snap, tz) } else { ops::card_summary(c, snap, tz) }
        }).collect::<Vec<_>>(),
    }))
}

pub async fn card_get(a: CardGetArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let full = is_full(&a.detail, true);
    let (c, snap) = ops::get_card(&r, &a.card, full).await?;
    let tz = ctx.tz();
    let mut v = if full {
        ops::card_detail(&c, snap.as_ref(), tz)
    } else {
        ops::card_summary(&c, snap.as_ref(), tz)
    };
    if a.include_comments.unwrap_or(false) {
        let list = ops::comment_list(&r, &c.id, Some(20)).await?;
        v["comments"] = json!(list
            .iter()
            .map(|x| ops::comment_json(x, tz))
            .collect::<Vec<_>>());
    }
    if a.include_activity.unwrap_or(false) {
        let acts = ctx.client.card_actions(&c.id, None, 0).await?;
        v["activity"] = json!(acts
            .iter()
            .take(20)
            .map(|x| json!({ "type": x.action_type, "date": x.date, "by": x.author(), "data": x.data }))
            .collect::<Vec<_>>());
    }
    Ok(v)
}

fn write_out(
    ctx: &Ctx,
    out: &ops::WriteResult,
    snap: Option<&trelloc_core::model::BoardSnapshot>,
) -> Value {
    let mut v = ops::card_summary(&out.card, snap, ctx.tz());
    v["notes"] = json!(out.notes);
    v["warnings"] = json!(out.warnings);
    v
}

async fn snap_for(r: &Resolver<'_>, board_id: &str) -> Option<trelloc_core::model::BoardSnapshot> {
    r.snapshot(&trelloc_core::resolve::BoardId {
        id: board_id.to_string(),
        name: board_id.to_string(),
    })
    .await
    .ok()
}

fn fields_input(m: &Option<BTreeMap<String, Value>>) -> (BTreeMap<String, String>, Vec<String>) {
    let mut set = BTreeMap::new();
    let mut clear = Vec::new();
    for (k, v) in m.iter().flatten() {
        if v.is_null() {
            clear.push(k.clone());
        } else {
            set.insert(k.clone(), value_to_input(v));
        }
    }
    (set, clear)
}

/// `null` → `"none"` (clear); anything else → human string.
fn date_input(v: &Option<Value>) -> Option<String> {
    v.as_ref().map(|v| match v {
        Value::Null => "none".to_string(),
        other => value_to_input(other),
    })
}

pub fn card_input(a: &CardCreateArgs) -> AppResult<ops::CardInput> {
    let (fields, clear) = fields_input(&a.custom_fields);
    if !clear.is_empty() {
        return Err(AppError::validation(
            "custom_fields values can't be null on create",
        ));
    }
    Ok(ops::CardInput {
        name: a.name.clone(),
        list: a.list.clone(),
        board: a.board.clone(),
        desc: a.desc.clone(),
        pos: a.pos.clone(),
        members: strs(&a.members),
        labels: strs(&a.labels),
        due: a.due.clone(),
        start: a.start.clone(),
        fields,
        checklists: a
            .checklists
            .iter()
            .flatten()
            .map(|c| ops::ChecklistInput {
                name: c.name.clone(),
                items: c.items.clone(),
            })
            .collect(),
        copy_from: a.copy_from.clone(),
        keep: a.keep.clone(),
    })
}

pub async fn card_create(a: CardCreateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let out = ops::create_card(&r, &card_input(&a)?).await?;
    let snap = snap_for(&r, &out.card.id_board).await;
    Ok(write_out(&ctx, &out, snap.as_ref()))
}

pub fn update_input(a: &CardUpdateArgs) -> ops::UpdateInput {
    let (fields, fields_clear) = fields_input(&a.custom_fields);
    ops::UpdateInput {
        name: a.name.clone(),
        desc: a.desc.clone(),
        due: date_input(&a.due),
        start: date_input(&a.start),
        due_complete: a.due_complete,
        members_add: strs(&a.members_add),
        members_remove: strs(&a.members_remove),
        labels_add: strs(&a.labels_add),
        labels_remove: strs(&a.labels_remove),
        fields,
        fields_clear,
        subscribed: a.subscribed,
        cover: None,
        closed: None,
    }
}

pub async fn card_update(a: CardUpdateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let out = ops::update_card(&r, &a.card, &update_input(&a)).await?;
    let snap = snap_for(&r, &out.card.id_board).await;
    Ok(write_out(&ctx, &out, snap.as_ref()))
}

pub async fn card_move(a: CardMoveArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let out = ops::move_card(&r, &a.card, &a.list, a.pos.as_deref()).await?;
    let snap = snap_for(&r, &out.card.id_board).await;
    Ok(write_out(&ctx, &out, snap.as_ref()))
}

pub async fn card_copy(a: CardCopyArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let out = ops::copy_card(&r, &a.card, &a.list, a.name.as_deref(), a.keep.as_deref()).await?;
    let snap = snap_for(&r, &out.card.id_board).await;
    Ok(write_out(&ctx, &out, snap.as_ref()))
}

pub async fn card_archive(a: CardArchiveArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let c = ops::archive_card(&r, &a.card, a.unarchive.unwrap_or(false)).await?;
    let snap = snap_for(&r, &c.id_board).await;
    Ok(ops::card_summary(&c, snap.as_ref(), ctx.tz()))
}

pub async fn card_delete(a: CardConfirmArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    let id = ops::delete_card(&ctx.resolver(), &a.card).await?;
    Ok(json!({ "deleted": true, "id": id }))
}

// ── collaboration ───────────────────────────────────────────────────────

pub async fn comment_list(a: CommentListArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let list = ops::comment_list(&ctx.resolver(), &a.card, a.limit).await?;
    Ok(json!(list
        .iter()
        .map(|c| ops::comment_json(c, ctx.tz()))
        .collect::<Vec<_>>()))
}

pub async fn comment_add(a: CommentAddArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let c = ops::comment_add(&ctx.resolver(), &a.card, &a.text).await?;
    Ok(ops::comment_json(&c, ctx.tz()))
}

pub async fn comment_edit(a: CommentEditArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let c = ops::comment_edit(&ctx.resolver(), &a.card, &a.comment_id, &a.text).await?;
    Ok(ops::comment_json(&c, ctx.tz()))
}

pub async fn comment_delete(a: CommentDeleteArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    ctx.resolver()
        .ensure_action_active(a.comment_id.trim())
        .await?;
    ctx.client.delete_comment(a.comment_id.trim()).await?;
    Ok(json!({ "deleted": true, "comment_id": a.comment_id }))
}

pub async fn checklist_list(a: CardArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let lists = ops::checklist_list(&ctx.resolver(), &a.card).await?;
    Ok(json!(lists
        .iter()
        .map(ops::checklist_json)
        .collect::<Vec<_>>()))
}

pub async fn checklist_add(a: ChecklistAddArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let id = r.card(&a.card).await?;
    let cl = ops::add_checklist(&r, &id, &a.name, &strs(&a.items)).await?;
    Ok(ops::checklist_json(&cl))
}

pub async fn checklist_item_add(a: ChecklistItemAddArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let item = ops::checklist_item_add(
        &ctx.resolver(),
        &a.card,
        &a.checklist,
        &a.text,
        a.due.as_deref(),
        a.member.as_deref(),
    )
    .await?;
    Ok(json!(item))
}

pub async fn checklist_item_update(a: ChecklistItemUpdateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let item = ops::checklist_item_update(
        &ctx.resolver(),
        &a.card,
        &a.item,
        &ops::ItemUpdate {
            checked: a.checked,
            text: a.text.clone(),
            due: a.due.clone(),
        },
    )
    .await?;
    Ok(json!(item))
}

pub async fn checklist_item_delete(a: ChecklistItemDeleteArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    let item = ops::checklist_item_delete(&ctx.resolver(), &a.card, &a.item).await?;
    Ok(json!({ "deleted": true, "item": item.name, "id": item.id }))
}

pub async fn checklist_delete(a: ChecklistDeleteArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    let cl = ops::checklist_delete(&ctx.resolver(), &a.card, &a.checklist).await?;
    Ok(json!({ "deleted": true, "checklist": cl.name, "id": cl.id }))
}

pub async fn label_list(a: BoardArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(
        ops::label_list(&ctx.resolver(), a.board.as_deref()).await?
    ))
}

pub async fn label_create(a: LabelCreateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(
        ops::label_create(&ctx.resolver(), a.board.as_deref(), &a.name, &a.color).await?
    ))
}

pub async fn label_update(a: LabelUpdateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(
        ops::label_update(
            &ctx.resolver(),
            a.board.as_deref(),
            &a.label,
            a.name.as_deref(),
            a.color.as_deref()
        )
        .await?
    ))
}

pub async fn label_delete(a: LabelDeleteArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    let l = ops::label_delete(&ctx.resolver(), a.board.as_deref(), &a.label).await?;
    Ok(json!({ "deleted": true, "label": l.display(), "id": l.id }))
}

pub async fn field_list(a: BoardArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let fields = ops::field_list(&ctx.resolver(), a.board.as_deref()).await?;
    Ok(json!(fields
        .iter()
        .map(|f| json!({
            "id": f.id, "name": f.name, "type": f.field_type,
            "options": f.options.iter().map(|o| json!({ "id": o.id, "name": o.value.text })).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>()))
}

pub async fn field_set(a: FieldSetArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let note = ops::set_card_field(
        &ctx.resolver(),
        &a.card,
        &a.field,
        &value_to_input(&a.value),
    )
    .await?;
    Ok(json!({ "result": note }))
}

pub async fn field_clear(a: FieldClearArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    let note = ops::clear_card_field(&ctx.resolver(), &a.card, &a.field).await?;
    Ok(json!({ "result": note }))
}

pub async fn attachment_list(a: CardArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(ops::attachment_list(&ctx.resolver(), &a.card).await?))
}

pub async fn attachment_add(a: AttachmentAddArgs) -> AppResult<Value> {
    let target = match (&a.path, &a.url) {
        (Some(p), None) => p.clone(),
        (None, Some(u)) => u.clone(),
        _ => return Err(AppError::validation("pass exactly one of `path` or `url`")),
    };
    let ctx = Ctx::load(&a.account)?;
    let att = ops::attachment_add(
        &ctx.resolver(),
        &a.card,
        &target,
        a.name.as_deref(),
        a.set_cover.unwrap_or(false),
    )
    .await?;
    Ok(json!(att))
}

pub async fn attachment_delete(a: AttachmentDeleteArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    ops::attachment_delete(&ctx.resolver(), &a.card, &a.attachment_id).await?;
    Ok(json!({ "deleted": true, "attachment_id": a.attachment_id }))
}

// ── bulk, management, reports ───────────────────────────────────────────

pub async fn card_bulk_create(a: BulkCreateArgs) -> AppResult<Value> {
    let inputs: Vec<ops::CardInput> = a
        .cards
        .iter()
        .map(|c| {
            let (fields, clear) = fields_input(&c.custom_fields);
            if !clear.is_empty() {
                return Err(AppError::validation(
                    "custom_fields values can't be null on create",
                ));
            }
            Ok(ops::CardInput {
                name: c.name.clone(),
                list: c.list.clone(),
                desc: c.desc.clone(),
                members: strs(&c.members),
                labels: strs(&c.labels),
                due: c.due.clone(),
                fields,
                ..Default::default()
            })
        })
        .collect::<AppResult<_>>()?;
    if inputs.is_empty() {
        return Err(AppError::validation("cards is empty"));
    }
    if a.dry_run.unwrap_or(false) {
        return Ok(json!({
            "dry_run": true,
            "would_create": inputs.iter().map(|c| json!({
                "name": c.name, "list": c.list.clone().or_else(|| a.list.clone()),
            })).collect::<Vec<_>>(),
        }));
    }
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let results = ops::bulk_create(&r, a.list.as_deref(), inputs, || {}).await;
    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
    Ok(json!({
        "total": results.len(),
        "ok": ok,
        "failed": results.len() - ok,
        "results": results.iter().map(|(name, r)| match r {
            Ok(w) => json!({ "name": name, "ok": true, "card": ops::card_summary(&w.card, None, ctx.tz()), "warnings": w.warnings }),
            Err(e) => json!({ "name": name, "ok": false, "error": e.to_string() }),
        }).collect::<Vec<_>>(),
    }))
}

fn changes(c: &BulkChangesArg) -> ops::BulkChanges {
    ops::BulkChanges {
        list: c.list.clone(),
        labels_add: strs(&c.labels_add),
        labels_remove: strs(&c.labels_remove),
        members_add: strs(&c.members_add),
        due: c.due.clone(),
        due_complete: c.due_complete,
        archive: c.archive,
        ..Default::default()
    }
}

pub async fn card_bulk_update(a: BulkUpdateArgs) -> AppResult<Value> {
    let ch = changes(&a.changes);
    if ch.is_empty() {
        return Err(AppError::validation("changes is empty"));
    }
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let cards: Vec<String> = match (&a.cards, &a.filter) {
        (Some(c), None) => c.clone(),
        (None, Some(f)) => {
            let mut input = search_input(f);
            input.limit = Some(
                f.limit
                    .unwrap_or(MAX_SEARCH_LIMIT)
                    .clamp(1, MAX_SEARCH_LIMIT),
            );
            ops::search_cards(&r, &input)
                .await?
                .cards
                .into_iter()
                .map(|c| c.id)
                .collect()
        }
        _ => {
            return Err(AppError::validation(
                "give exactly one of `cards` or `filter`",
            ))
        }
    };
    if a.dry_run.unwrap_or(false) || cards.is_empty() {
        return Ok(json!({
            "dry_run": true,
            "matched": cards.len(),
            "cards": cards,
            "changes": ch.describe(),
        }));
    }
    if cards.len() > 1 {
        require_confirm(a.confirm)?;
    }
    let results = ops::bulk_update(&r, cards, &ch, || {}).await?;
    Ok(ops::bulk_summary(&results))
}

pub async fn batch(a: BatchArgs) -> AppResult<Value> {
    let steps: Vec<ops::BatchOp> = a
        .ops
        .iter()
        .enumerate()
        .map(|(i, v)| {
            serde_json::from_value(v.clone())
                .map_err(|e| AppError::validation(format!("ops[{i}]: {e}")))
        })
        .collect::<AppResult<_>>()?;
    if steps.is_empty() {
        return Err(AppError::validation("ops is empty"));
    }
    if a.dry_run.unwrap_or(false) {
        return Ok(json!({
            "dry_run": true,
            "ops": steps.iter().map(ops::BatchOp::describe).collect::<Vec<_>>(),
        }));
    }
    if steps.len() > 1 || steps.iter().any(ops::BatchOp::is_destructive) {
        require_confirm(a.confirm)?;
    }
    let ctx = Ctx::load(&a.account)?;
    let r = ctx.resolver();
    let mut results = Vec::new();
    for step in &steps {
        results.push(match ops::run_batch_op(&r, step).await {
            Ok(v) => json!({ "op": step.describe(), "ok": true, "result": v }),
            Err(e) => json!({ "op": step.describe(), "ok": false, "error": AppError::from(e).to_mcp().message }),
        });
    }
    let failed = results.iter().filter(|v| v["ok"] == false).count();
    Ok(json!({ "total": results.len(), "failed": failed, "results": results }))
}

pub async fn board_create(a: BoardCreateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let b = ops::board_create(
        &ctx.resolver(),
        &a.name,
        a.account.workspace.as_deref(),
        a.from_board.as_deref(),
        a.default_lists.unwrap_or(true),
    )
    .await?;
    Ok(ops::board_summary(&b))
}

pub async fn board_update(a: BoardUpdateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let b = ops::board_update(
        &ctx.resolver(),
        &a.board,
        &ops::BoardUpdate {
            name: a.name.clone(),
            desc: a.desc.clone(),
            closed: a.closed,
        },
    )
    .await?;
    Ok(ops::board_summary(&b))
}

pub async fn board_delete(a: BoardDeleteArgs) -> AppResult<Value> {
    require_confirm(a.confirm)?;
    let ctx = Ctx::load(&a.account)?;
    let b = ops::board_delete(&ctx.resolver(), &a.board).await?;
    Ok(json!({ "deleted": true, "board": b }))
}

pub async fn list_create(a: ListCreateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(
        ops::list_create(
            &ctx.resolver(),
            a.board.as_deref(),
            &a.name,
            a.pos.as_deref()
        )
        .await?
    ))
}

pub async fn list_update(a: ListUpdateArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(
        ops::list_update(
            &ctx.resolver(),
            a.board.as_deref(),
            &a.list,
            &ops::ListUpdate {
                name: a.name.clone(),
                pos: a.pos.clone(),
                closed: a.closed,
            },
        )
        .await?
    ))
}

pub async fn standup(a: StandupArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    let st = ops::standup(
        &ctx.resolver(),
        a.since.as_deref().unwrap_or("yesterday"),
        a.board.as_deref(),
        a.member.as_deref(),
    )
    .await?;
    Ok(json!(st))
}

pub async fn board_summary(a: BoardArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(ops::board_report(&ctx.resolver(), a.board.as_deref()).await?)
}

pub async fn activity(a: ActivityArgs) -> AppResult<Value> {
    let ctx = Ctx::load(&a.account)?;
    Ok(json!(
        ops::activity(
            &ctx.resolver(),
            &ops::ActivityInput {
                board: a.board.clone(),
                card: a.card.clone(),
                since: a.since.clone(),
                member: a.member.clone(),
                limit: a.limit,
            },
        )
        .await?
    ))
}
