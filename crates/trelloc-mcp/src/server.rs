use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::{
        stdio,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
    ErrorData, Json, ServerHandler, ServiceExt,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{app, error::AppResult, models::*};

#[derive(Clone)]
pub struct TrelloMcpServer {
    tool_router: ToolRouter<Self>,
}

impl TrelloMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for TrelloMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// MCP stdio frames are single JSON lines and several clients cap line
/// length (e.g. 64 KB); keep responses comfortably below that.
const MAX_RESPONSE_BYTES: usize = 48 * 1024;
const MAX_STRING_CHARS: usize = 2000;

fn bound_response(mut value: Value) -> Value {
    if serde_json::to_string(&value).map_or(0, |s| s.len()) <= MAX_RESPONSE_BYTES {
        return value;
    }
    truncate_long_strings(&mut value, MAX_STRING_CHARS);
    if let Value::Object(map) = &mut value {
        map.insert("_truncated".into(), Value::Bool(true));
    }
    value
}

fn truncate_long_strings(value: &mut Value, max: usize) {
    match value {
        Value::String(text) if text.chars().count() > max => {
            let kept: String = text.chars().take(max).collect();
            *text = format!("{kept}…[truncated]");
        }
        Value::Array(items) => items.iter_mut().for_each(|v| truncate_long_strings(v, max)),
        Value::Object(map) => map.values_mut().for_each(|v| truncate_long_strings(v, max)),
        _ => {}
    }
}

fn respond(result: AppResult<Value>) -> Result<Json<ToolResponse>, ErrorData> {
    result
        .map(|v| {
            Json(ToolResponse {
                result: bound_response(v),
            })
        })
        .map_err(|e| e.to_mcp())
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TrelloMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("trelloc", env!("CARGO_PKG_VERSION"))
                .with_title("trelloc MCP")
                .with_description("Unofficial Trello tools for MCP clients"),
        )
        .with_instructions(
            "Use the trello_* tools for Trello boards, lists, cards, comments, checklists, labels, custom fields, attachments and raw API access. Inputs accept names (boards, lists, labels, members, fields, dropdown options) or ids, and dates like 'tomorrow' or '2026-10-20'. Cards accept an id, shortLink, card URL or 'board#42'. Every tool takes optional `profile` (account, see trello_profile_list) and `workspace` to narrow board names. Delete/remove/clear tools require confirm=true; archiving is the safe default.",
        )
    }
}

#[tool_router(router = tool_router)]
impl TrelloMcpServer {
    // ── auth & meta ─────────────────────────────────────────────────────

    #[tool(
        name = "trello_auth_status",
        description = "Show the active profile, the cached member, whether an API key and token are configured (masked), the default board and the config path. No API call. Use first when other tools return auth_missing."
    )]
    pub async fn trello_auth_status(
        &self,
        Parameters(args): Parameters<Account>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::auth_status(args))
    }

    #[tool(
        name = "trello_whoami",
        description = "The logged-in Trello member (id, username, full name), cached at login. No API call."
    )]
    pub async fn trello_whoami(
        &self,
        Parameters(args): Parameters<Account>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::whoami(args))
    }

    #[tool(
        name = "trello_profile_list",
        description = "List the trelloc profiles (Trello accounts) configured on this machine and which one is active. Pass `profile` to any trello_* tool to run it as another account."
    )]
    pub async fn trello_profile_list(&self) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::profile_list())
    }

    // ── hierarchy ───────────────────────────────────────────────────────

    #[tool(
        name = "trello_workspace_list",
        description = "List every Trello workspace (organization) the token can see. Use a workspace's name in `workspace` on other tools to disambiguate boards with the same name."
    )]
    pub async fn trello_workspace_list(
        &self,
        Parameters(args): Parameters<Account>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::workspace_list(args).await)
    }

    #[tool(
        name = "trello_board_list",
        description = "List boards the token can see (open by default), optionally within one `workspace` or only starred ones. Returns ids, shortLinks and URLs."
    )]
    pub async fn trello_board_list(
        &self,
        Parameters(args): Parameters<BoardListArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::board_list(args).await)
    }

    #[tool(
        name = "trello_board_get",
        description = "One board's schema in one call: lists (with open-card counts), labels, members and custom fields with dropdown options. Call this before creating or moving cards to learn valid list, label, member and field names."
    )]
    pub async fn trello_board_get(
        &self,
        Parameters(args): Parameters<BoardArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::board_get(args).await)
    }

    #[tool(
        name = "trello_member_list",
        description = "Members of a board (default: the profile's default board) or of a workspace (`in_workspace`). Use the usernames as member inputs on card tools."
    )]
    pub async fn trello_member_list(
        &self,
        Parameters(args): Parameters<MemberListArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::member_list(args).await)
    }

    // ── cards ───────────────────────────────────────────────────────────

    #[tool(
        name = "trello_card_search",
        description = "Find cards. With `boards` or `lists` it scans those boards exactly (preferred, sees new cards immediately). With only `members: [\"me\"]` it reads your cards everywhere. `query` uses Trello search syntax and may lag a few seconds; the response's `mode` says which path ran. Filters: members, labels, due ranges, overdue, no_due, due_complete, created/active dates, has, custom `fields` (need a board), text. Returns compact summaries with `ref` (board#42) usable as `card` elsewhere; `detail: \"full\"` adds description, fields, checklists."
    )]
    pub async fn trello_card_search(
        &self,
        Parameters(args): Parameters<CardSearchArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_search(args).await)
    }

    #[tool(
        name = "trello_card_get",
        description = "One card in full: board, list, members, labels, dates, description, custom field values, checklists with items, attachments. `card` accepts an id, shortLink, card URL or \"board#42\". Optionally include the latest comments and activity."
    )]
    pub async fn trello_card_get(
        &self,
        Parameters(args): Parameters<CardGetArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_get(args).await)
    }

    #[tool(
        name = "trello_card_create",
        description = "Create a card in a list. `list` is a name on `board` (or the default board) or \"Board/List\". Members, labels, custom field values and dropdown options are given by name; dates like \"fri\" work. Checklists with items and a copy source are optional. Check `warnings` for parts that failed (e.g. a custom field)."
    )]
    pub async fn trello_card_create(
        &self,
        Parameters(args): Parameters<CardCreateArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_create(args).await)
    }

    #[tool(
        name = "trello_card_update",
        description = "Change a card. Members and labels are added/removed individually (members_add, labels_remove, …), never by replacing the whole set. `due: null` clears the due date. Custom fields by name; null clears one. To move between lists use trello_card_move; to archive use trello_card_archive."
    )]
    pub async fn trello_card_update(
        &self,
        Parameters(args): Parameters<CardUpdateArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_update(args).await)
    }

    #[tool(
        name = "trello_card_move",
        description = "Move a card to another list (status change) on the same board, or to \"Board/List\" on another board. Labels, members and custom fields are board-scoped: cross-board moves map labels by name and return `warnings` for anything that will be dropped — tell the user."
    )]
    pub async fn trello_card_move(
        &self,
        Parameters(args): Parameters<CardMoveArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_move(args).await)
    }

    #[tool(
        name = "trello_card_archive",
        description = "Archive a card (reversible; the safe way to remove a card) or restore it with unarchive=true. No confirmation needed."
    )]
    pub async fn trello_card_archive(
        &self,
        Parameters(args): Parameters<CardArchiveArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_archive(args).await)
    }

    #[tool(
        name = "trello_card_delete",
        description = "Permanently delete a card. Cannot be undone; prefer trello_card_archive. Requires confirm=true."
    )]
    pub async fn trello_card_delete(
        &self,
        Parameters(args): Parameters<CardConfirmArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::card_delete(args).await)
    }

    // ── collaboration ───────────────────────────────────────────────────

    #[tool(
        name = "trello_comment_list",
        description = "Latest comments on a card, newest first, with ids for editing."
    )]
    pub async fn trello_comment_list(
        &self,
        Parameters(args): Parameters<CommentListArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::comment_list(args).await)
    }

    #[tool(
        name = "trello_comment_add",
        description = "Add a Markdown comment to a card. \"@username\" mentions notify that member."
    )]
    pub async fn trello_comment_add(
        &self,
        Parameters(args): Parameters<CommentAddArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::comment_add(args).await)
    }

    #[tool(
        name = "trello_comment_edit",
        description = "Replace the text of one of your comments (comment_id from trello_comment_list)."
    )]
    pub async fn trello_comment_edit(
        &self,
        Parameters(args): Parameters<CommentEditArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::comment_edit(args).await)
    }

    #[tool(
        name = "trello_comment_delete",
        description = "Delete a comment. Requires confirm=true."
    )]
    pub async fn trello_comment_delete(
        &self,
        Parameters(args): Parameters<CommentDeleteArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::comment_delete(args).await)
    }

    #[tool(
        name = "trello_checklist_list",
        description = "Checklists on a card with done/total progress and item ids."
    )]
    pub async fn trello_checklist_list(
        &self,
        Parameters(args): Parameters<CardArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::checklist_list(args).await)
    }

    #[tool(
        name = "trello_checklist_add",
        description = "Add a checklist to a card, optionally with items."
    )]
    pub async fn trello_checklist_add(
        &self,
        Parameters(args): Parameters<ChecklistAddArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::checklist_add(args).await)
    }

    #[tool(
        name = "trello_checklist_item_add",
        description = "Add an item to a checklist (by checklist name or id), optionally with a due date and member."
    )]
    pub async fn trello_checklist_item_add(
        &self,
        Parameters(args): Parameters<ChecklistItemAddArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::checklist_item_add(args).await)
    }

    #[tool(
        name = "trello_checklist_item_update",
        description = "Check/uncheck, rename or re-date a checklist item. `item` may be the id, the exact text, or a unique part of it."
    )]
    pub async fn trello_checklist_item_update(
        &self,
        Parameters(args): Parameters<ChecklistItemUpdateArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::checklist_item_update(args).await)
    }

    #[tool(
        name = "trello_checklist_item_delete",
        description = "Delete a checklist item. Requires confirm=true."
    )]
    pub async fn trello_checklist_item_delete(
        &self,
        Parameters(args): Parameters<ChecklistItemDeleteArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::checklist_item_delete(args).await)
    }

    #[tool(
        name = "trello_checklist_delete",
        description = "Delete a whole checklist from a card. Requires confirm=true."
    )]
    pub async fn trello_checklist_delete(
        &self,
        Parameters(args): Parameters<ChecklistDeleteArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::checklist_delete(args).await)
    }

    #[tool(
        name = "trello_label_list",
        description = "Labels on a board (name, color, id). Labels are board-scoped."
    )]
    pub async fn trello_label_list(
        &self,
        Parameters(args): Parameters<BoardArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::label_list(args).await)
    }

    #[tool(
        name = "trello_label_create",
        description = "Create a label on a board."
    )]
    pub async fn trello_label_create(
        &self,
        Parameters(args): Parameters<LabelCreateArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::label_create(args).await)
    }

    #[tool(
        name = "trello_label_update",
        description = "Rename or recolor a board label."
    )]
    pub async fn trello_label_update(
        &self,
        Parameters(args): Parameters<LabelUpdateArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::label_update(args).await)
    }

    #[tool(
        name = "trello_label_delete",
        description = "Delete a label from the board and every card using it. Requires confirm=true."
    )]
    pub async fn trello_label_delete(
        &self,
        Parameters(args): Parameters<LabelDeleteArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::label_delete(args).await)
    }

    #[tool(
        name = "trello_field_list",
        description = "Custom field definitions on a board with dropdown options. Returns `unavailable` when the Custom Fields Power-Up is off."
    )]
    pub async fn trello_field_list(
        &self,
        Parameters(args): Parameters<BoardArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::field_list(args).await)
    }

    #[tool(
        name = "trello_field_set",
        description = "Set a custom field on a card by field name: dropdown option name, number, date, true/false or text."
    )]
    pub async fn trello_field_set(
        &self,
        Parameters(args): Parameters<FieldSetArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::field_set(args).await)
    }

    #[tool(
        name = "trello_field_clear",
        description = "Clear a custom field value on a card. Requires confirm=true."
    )]
    pub async fn trello_field_clear(
        &self,
        Parameters(args): Parameters<FieldClearArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::field_clear(args).await)
    }

    #[tool(
        name = "trello_attachment_list",
        description = "Attachments on a card (name, url, size)."
    )]
    pub async fn trello_attachment_list(
        &self,
        Parameters(args): Parameters<CardArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::attachment_list(args).await)
    }

    #[tool(
        name = "trello_attachment_add",
        description = "Attach a local file (`path`, on the machine running the server) or a URL (`url`) to a card."
    )]
    pub async fn trello_attachment_add(
        &self,
        Parameters(args): Parameters<AttachmentAddArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::attachment_add(args).await)
    }

    #[tool(
        name = "trello_attachment_delete",
        description = "Delete an attachment from a card. Requires confirm=true."
    )]
    pub async fn trello_attachment_delete(
        &self,
        Parameters(args): Parameters<AttachmentDeleteArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::attachment_delete(args).await)
    }

    // ── escape hatch ────────────────────────────────────────────────────

    #[tool(
        name = "trello_api_request",
        description = "Raw Trello REST call for anything the other tools don't cover. `path` is relative to /1 (e.g. /boards/{id}/actions); credentials are added as a header, never pass key/token. Non-GET requires confirm=true. Prefer the dedicated tools: they resolve names and pace requests."
    )]
    pub async fn trello_api_request(
        &self,
        Parameters(args): Parameters<ApiRequestArgs>,
    ) -> Result<Json<ToolResponse>, ErrorData> {
        respond(app::api_request(args).await)
    }
}

pub async fn run_stdio() -> anyhow::Result<()> {
    let server = TrelloMcpServer::new().serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}

pub async fn run_streamable_http(host: &str, port: u16, path: &str) -> anyhow::Result<()> {
    let cancellation_token = CancellationToken::new();
    let allowed_hosts = [
        format!("{host}:{port}"),
        host.to_string(),
        format!("127.0.0.1:{port}"),
        "127.0.0.1".to_string(),
        format!("localhost:{port}"),
        "localhost".to_string(),
    ];
    let service = StreamableHttpService::new(
        || Ok(TrelloMcpServer::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation_token.child_token())
            .with_allowed_hosts(allowed_hosts),
    );
    let router = axum::Router::new().nest_service(path, service);
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancellation_token.cancel();
        })
        .await?;
    Ok(())
}
