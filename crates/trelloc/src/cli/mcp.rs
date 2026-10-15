//! `trelloc mcp serve` (install and doctor arrive later).

use anyhow::Result;
use clap::{Subcommand, ValueEnum};

use super::Ctx;

#[derive(Debug, Subcommand)]
pub enum McpCmd {
    /// Run the MCP server over stdio (default) or Streamable HTTP
    Serve {
        #[arg(long, value_enum, default_value = "stdio")]
        transport: Transport,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8787)]
        port: u16,
        #[arg(long, default_value = "/mcp")]
        path: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Transport {
    Stdio,
    StreamableHttp,
}

pub async fn run(_ctx: &Ctx, cmd: McpCmd) -> Result<()> {
    match cmd {
        McpCmd::Serve {
            transport,
            host,
            port,
            path,
        } => match transport {
            Transport::Stdio => trelloc_mcp::run_stdio().await,
            Transport::StreamableHttp => trelloc_mcp::run_streamable_http(&host, port, &path).await,
        },
    }
}

pub async fn interactive_install(_ctx: &Ctx) -> Result<()> {
    println!("Run `trelloc mcp install --client <client>` to register the server.");
    Ok(())
}
