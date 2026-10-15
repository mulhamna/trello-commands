use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler, tool_router,
    transport::{
        stdio,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
    ServerHandler, ServiceExt,
};
use tokio_util::sync::CancellationToken;

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

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TrelloMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("trelloc", env!("CARGO_PKG_VERSION"))
                .with_title("trelloc MCP")
                .with_description("Unofficial Trello tools for MCP clients"),
        )
    }
}

#[tool_router(router = tool_router)]
impl TrelloMcpServer {}

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
