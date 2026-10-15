# Architecture

```
            ┌──────────────── trelloc (one binary) ────────────────┐
            │ ┌──────────────┐          ┌───────────────────────┐ │
 user ────► │ │  CLI cmds    │          │ `trelloc mcp serve`   │ │ ◄── MCP client (Claude, Cursor, …)
            │ │  (clap)      │          │ trelloc-mcp (rmcp)    │ │
            │ └──────┬───────┘          └──────────┬────────────┘ │
            └────────┼─────────────────────────────┼──────────────┘
                     │        trelloc_core::ops     │
                     ▼                              ▼
            ┌─────────────────────────────────────────────┐
            │                trelloc-core                 │
            │  ops ─► Resolver ─► TrelloClient ─► Limiter │
            │        (names→ids,  (REST /1)      (bucket, │
            │         cache)                      429)    │
            └─────────────────────┬───────────────────────┘
                                  ▼
                      https://api.trello.com/1
```

One rule: **the CLI and MCP crates never see a URL or a raw HTTP status.** They call `trelloc_core::ops` with human input and get typed models or `TrelloError` back.

---

## trelloc-core

### Config — `config.rs`

File: `~/.config/trelloc/config.toml`, chmod 600 warning, multi-profile.

```toml
current_profile = "work"

[profiles.work]
api_key = "…"               # optional if TRELLO_API_KEY is set
token = "…"                 # optional if TRELLO_TOKEN is set
member_id = "5f…"           # cached at login (avoids /1/members/me calls)
username = "mulham"
workspace = "6a…"           # optional default workspace (organization id) for board lookups
default_board = "AbCd1234"  # optional; used when --board omitted
default_list = "To Do"      # optional; used when --list omitted on create (resolved within default_board)
timezone = "Asia/Jakarta"   # optional; default: system
timeout_secs = 30
default_card_limit = 200

[profiles.work.aliases]     # short names → board/list refs
roadmap = "AbCd1234"
inbox   = "AbCd1234/Inbox"
```

Env overrides (highest priority): `TRELLO_API_KEY`, `TRELLO_TOKEN`, `TRELLO_PROFILE`, `TRELLO_WORKSPACE`, `TRELLO_DEFAULT_BOARD`, `TRELLO_TIMEOUT_SECS`, `TRELLO_CARD_LIMIT`, `TRELLO_BASE_URL` (tests), `TRELLO_CACHE_DIR`.

### Errors — `error.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum TrelloError {
    #[error("HTTP error: {0}")]            Http(#[from] reqwest::Error),
    #[error("Authentication error: {0}")]  Auth(String),          // 401, missing key/token
    #[error("API error {status}: {message}")]
    Api { status: u16, code: Option<String>, message: String },   // code = e.g. API_TOKEN_DB_LIMIT_EXCEEDED
    #[error("Not found: {0}")]             NotFound(String),
    #[error("Rate limited ({limit})")]     RateLimit { limit: String },
    #[error("Feature unavailable: {0}")]   Unavailable(String),   // 403 Custom Fields disabled / plan
    #[error("Configuration error: {0}")]   Config(String),
    #[error("Could not resolve {kind} '{input}'{}", suggestion_hint(.suggestions))]
    Resolve { kind: &'static str, input: String, suggestions: Vec<String> },
    #[error("Serialization error: {0}")]   Serialization(#[from] serde_json::Error),
    #[error("IO error: {0}")]              Io(#[from] std::io::Error),
}
```

### HTTP client — `client/mod.rs`

```rust
pub struct TrelloClient {
    http: reqwest::Client,          // default header: Authorization: OAuth oauth_consumer_key=…, oauth_token=…
    base: Url,                      // https://api.trello.com/1
    limiter: Arc<RateLimiter>,
}

impl TrelloClient {
    async fn get<T>(&self, path: &str, q: &Query) -> Result<T>;
    async fn post<T>(&self, path: &str, q: &Query, body: Option<&Value>) -> Result<T>;
    async fn put<T>(&self, path: &str, q: &Query, body: Option<&Value>) -> Result<T>;
    async fn delete(&self, path: &str, q: &Query) -> Result<()>;
    async fn multipart<T>(&self, path: &str, form: Form) -> Result<T>;
    pub async fn batch(&self, urls: &[String]) -> Result<Vec<Result<Value>>>;   // chunks of 10
    pub async fn raw(&self, method: Method, path: &str, q: Option<Value>, body: Option<Value>) -> Result<Value>; // `trelloc api`
}
```

- Credentials only in the `Authorization` header (marked sensitive with `HeaderValue::set_sensitive(true)`).
- `send_with_retry` = limiter permit → send → `observe(headers)` → on 429 backoff loop.
- Error bodies: JSON `{message|error}` or plain text.
- Write params (including long `desc` and comment `text`) go in a JSON body, never the URL.

Resource methods in `client/{member,board,list,card,comment,checklist,label,field,attachment,action}.rs`.

### Rate limiter — `rate_limit.rs`

Trello gives a 10 s rolling window and no reset timestamp, so:

```rust
pub struct RateLimiter {
    token: Bucket,     // 90 / 10s (configurable)
    members: Bucket,   // 90 / 900s, only for top-level /1/members/* routes
    search: Bucket,    // conservative, e.g. 10 / 10s
    permits: Semaphore // bounded concurrency for bulk (default 4)
}
impl RateLimiter {
    pub async fn acquire(&self, route: RouteClass) -> Permit;
    pub fn observe(&self, headers: &HeaderMap);   // x-rate-limit-api-token-remaining / -interval-ms
}
```

`RouteClass::for_path(path)` decides which buckets apply (`/members/me` → token + members; `/members/me/boards` → token only; `/search` → token + search).

### Refs — `card_ref.rs`, `board_ref.rs`

```rust
pub enum CardRef { Id(String), ShortLink(String), BoardNumber { board: String, number: u32 } }
pub enum BoardRef { Id(String), ShortLink(String), Name(String) }
```

Card inputs:
- `5f2b…` (24 hex) → `Id`
- `AbCd1234` (8 chars `[A-Za-z0-9]`) → `ShortLink` (usable directly as id)
- `https://trello.com/c/AbCd1234/42-fix-login` → `ShortLink("AbCd1234")`
- `roadmap#42`, `Roadmap#42` (board name/alias/shortLink + `#` + number) → `BoardNumber` → resolved by reading the board's `id,idShort` card list (no by-number route in the spec)
- `#42` alone → `BoardNumber` on `default_board`

Board inputs: id, shortLink, `https://trello.com/b/AbCd1234/name`, alias, or name (via resolver).

Ambiguity rule: an 8-char alphanumeric string is tried as a shortLink first; on 404 fall back to name resolution. Fuzz these parsers (`fuzz/` `parsers` target).

### Resolver — `resolve.rs`

```rust
impl Resolver<'_> {
    pub async fn workspace(&self, input: &str) -> Result<WorkspaceRef>;
    pub async fn board(&self, input: &str) -> Result<BoardRef>;           // id | shortLink | URL | alias | name (fuzzy)
    pub async fn snapshot(&self, board: &BoardRef) -> Result<BoardSnapshot>; // lists, labels, members, fields (cached)
    pub async fn list(&self, board: &BoardRef, input: &str) -> Result<ListRef>; // name | id | position "1"
    pub async fn list_path(&self, input: &str) -> Result<ListRef>;        // "Board/List" or alias
    pub async fn label(&self, board: &BoardRef, input: &str) -> Result<Label>;   // name | color | id
    pub async fn member(&self, board: &BoardRef, input: &str) -> Result<Member>; // "me" | username | full name | id
    pub async fn field(&self, board: &BoardRef, input: &str) -> Result<CustomField>;
    pub async fn field_value(&self, field: &CustomField, raw: &str) -> Result<FieldValue>;
    pub async fn card(&self, input: &str) -> Result<CardId>;              // via card_ref
}
```

Cache: `~/.cache/trelloc/{member_id}/boards.json` (my boards + workspaces) and `snapshots/{board_id}.json`, TTL 1 h, `--refresh` / `trelloc cache clear`. Refresh once on a miss before failing. "me" never calls the API (cached member id).

Ambiguous board names (same name in two workspaces) → `Resolve` error listing `Workspace/Board`.

### Filters — `filter.rs`

```rust
pub struct CardFilter {
    pub boards: Vec<String>, pub lists: Vec<String>,          // resolved ids
    pub members: Vec<String>, pub labels: Vec<String>,
    pub due: Range, pub created: Range, pub activity: Range,  // Range { after, before } as ISO/ms
    pub overdue: bool, pub no_due: bool,
    pub due_complete: Option<bool>,
    pub include_archived: bool,
    pub has: Vec<Has>,                                        // attachments, description, members, cover
    pub fields: Vec<FieldFilter>,                             // { field_id, op, value }
    pub text: Option<String>,                                 // name/desc contains
    pub raw_query: Option<String>,                            // user-supplied Trello search syntax
    pub sort: Option<Sort>, pub limit: Option<usize>,
}
impl CardFilter {
    pub fn plan(&self) -> Plan;                     // BoardScan { boards } | Search { query, id_boards }
    pub fn matches(&self, card: &Card) -> bool;     // client-side predicate for BoardScan
    pub fn to_search_query(&self) -> String;        // operators for Search
}
```

Plan selection (see TRELLO-API §11): any board/list scope and no `raw_query` → `BoardScan` (exact, cheap); otherwise `Search`. With `--mine` and no scope → `GET /1/members/me/cards` + predicate.

### Ops — `ops.rs`

Human-input operations shared by CLI and MCP: `search_cards`, `create_card`, `update_card`, `move_card`, `copy_card`, `archive_card`, `delete_card`, `set_field`, `add_comment`, `checklist_*`, `standup`, `board_summary`, `run_batch_op`, `card_summary(&Card) -> Value`.

`move_card` across boards: compute which labels/members/fields don't exist on the target board, return them as `warnings` (CLI prints, MCP returns) and map labels by name when possible.

### Time — `time.rs`

`parse_when(input, tz) -> DateTime<Utc>` (grammar: `2026-10-20`, `tomorrow`, `fri`, `+3d`, `eow`, `none`) and `to_trello_iso()`. `created_at(id) -> DateTime` from the id's first 8 hex chars.

### Models — `model.rs`

Tolerant serde (`#[serde(default)]`, unknown fields ignored). `Card` keeps `labels` (objects) and `idMembers`; display joins members via the snapshot. `CustomFieldItem.value` kept as `Value` + `display_value(&CustomField)`.

---

## trelloc (CLI)

clap derive, global `--profile`, `--json`, `-v`, `--refresh`, `-y/--yes`; `inquire` pickers on TTY (board → list → card); `comfy-table` output; exit codes 0 ok / 1 error / 2 usage / 3 auth / 4 not found / 5 rate limited. Spec: `docs/CLI.md`.

## trelloc-mcp

Library with `run_stdio` / `run_streamable_http`, tools prefixed `trello_`, `AppError` codes (`validation_error`, `unsafe_operation`, `auth_missing`, `config_error`, `resolve_error`, `trello_api_error`, `rate_limited`, `unavailable`), per-tool optional `profile`, stdout clean in stdio mode. Spec: `docs/MCP.md`.

## Testing

- Unit: refs parsers, filter planning + predicate + query building, time parsing, id→created date, resolver fuzzy matching.
- wiremock per client method: happy path, 404 → `NotFound`, 429 then success, 429 exhausted → `RateLimit`, plain-text and JSON error bodies. Assert the `Authorization: OAuth …` header and that **no request URL contains `key=` or `token=`**.
- Limiter: simulated clock tests for the 10 s window and the members 900 s bucket.
- MCP: in-process handler tests + error mapping test.
- Fuzz target for `card_ref` / `board_ref` / `time` parsers.
- Optional live smoke test behind `TRELLO_LIVE_TEST=1` against a scratch board; never in CI.
