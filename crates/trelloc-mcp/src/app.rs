//! Tool implementations: validate → resolve names → call core → shape output.
//! Config is reloaded per call so `trelloc auth login` takes effect without
//! restarting the server; the rate limiter is shared per token.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use serde_json::{json, Value};
use trelloc_core::{
    config::{mask_secret, ConfigFile, TrelloConfig},
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
    workspace: Option<String>,
}

impl Ctx {
    pub fn load(acc: &Account) -> AppResult<Self> {
        let cfg = load_config(acc.profile.as_deref())?;
        let (_, token) = cfg.credentials()?;
        let client = TrelloClient::new(&cfg)?.with_limiter(limiter_for(token));
        Ok(Self {
            cfg,
            client,
            workspace: acc.workspace.clone().filter(|w| !w.trim().is_empty()),
        })
    }

    pub fn resolver(&self) -> Resolver<'_> {
        Resolver::new(&self.client, &self.cfg).with_workspace(self.workspace.clone())
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
