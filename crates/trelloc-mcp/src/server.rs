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

use crate::{
    app,
    error::AppResult,
    models::{Account, ApiRequestArgs, BoardArgs, BoardListArgs, MemberListArgs, ToolResponse},
};

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
