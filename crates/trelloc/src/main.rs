use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Debug, Parser)]
#[command(
    name = "trelloc",
    about = "Unofficial Trello CLI and MCP server",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Debug logging to stderr
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// MCP server
    #[command(subcommand)]
    Mcp(McpCmd),
}

#[derive(Debug, Subcommand)]
enum McpCmd {
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
enum Transport {
    Stdio,
    StreamableHttp,
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Mcp(McpCmd::Serve {
            transport,
            host,
            port,
            path,
        }) => match transport {
            Transport::Stdio => trelloc_mcp::run_stdio().await,
            Transport::StreamableHttp => trelloc_mcp::run_streamable_http(&host, port, &path).await,
        },
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let filter = if cli.verbose {
        EnvFilter::new("trelloc=debug,trelloc_core=debug,trelloc_mcp=debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
