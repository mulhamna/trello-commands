mod cli;
mod help_text;
mod output;

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, EnvFilter};
use trelloc_core::TrelloError;

use cli::{
    auth::{AuthCmd, ConfigCmd, WorkspaceCmd},
    hierarchy::{AliasCmd, BoardCmd, CacheCmd, ListCmd, MemberCmd},
    mcp::McpCmd,
};

#[derive(Debug, Parser)]
#[command(
    name = "trelloc",
    about = "Unofficial Trello CLI and MCP server",
    long_about = help_text::ROOT_LONG_ABOUT,
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Config profile to use (default: TRELLO_PROFILE or current_profile)
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Workspace name or id to narrow board names for this command
    #[arg(long, global = true)]
    workspace: Option<String>,

    /// Machine-readable JSON output
    #[arg(long, global = true)]
    json: bool,

    /// Debug logging to stderr
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Ignore cached boards and board snapshots
    #[arg(long, global = true)]
    refresh: bool,

    /// Skip confirmations
    #[arg(short = 'y', long, global = true)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Log in, check status, manage profiles
    #[command(subcommand, long_about = help_text::AUTH_LONG_ABOUT)]
    Auth(AuthCmd),
    /// The logged-in member (cached; --refresh asks Trello)
    Whoami,
    /// List workspaces or set the default one
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
    /// List, view and pick boards
    #[command(subcommand)]
    Board(BoardCmd),
    /// Workspace → Board → List tree
    Tree,
    /// Lists on a board
    #[command(subcommand)]
    List(ListCmd),
    /// Short names for boards and lists
    #[command(subcommand)]
    Alias(AliasCmd),
    /// Board and workspace members
    #[command(subcommand)]
    Member(MemberCmd),
    /// Clear cached boards and board snapshots
    #[command(subcommand)]
    Cache(CacheCmd),
    /// Raw API request, e.g. `trelloc api GET /members/me/boards --query fields=name`
    Api {
        method: String,
        /// Path relative to /1, e.g. /boards/{id}/lists
        path: String,
        /// Query string, e.g. 'fields=name,url&filter=open'
        #[arg(long)]
        query: Option<String>,
        /// JSON body, or @file.json
        #[arg(long)]
        data: Option<String>,
    },
    /// Read or change profile settings
    #[command(subcommand)]
    Config(ConfigCmd),
    /// MCP server: serve, install, doctor
    #[command(subcommand, long_about = help_text::MCP_LONG_ABOUT)]
    Mcp(McpCmd),
}

async fn run(cli: Cli) -> Result<()> {
    let ctx = cli::Ctx {
        profile: cli.profile,
        workspace: cli.workspace,
        json: cli.json,
        yes: cli.yes,
        refresh: cli.refresh,
    };
    if let Some(p) = &ctx.profile {
        // The MCP server path loads config itself; make the flag visible there too.
        std::env::set_var("TRELLO_PROFILE", p);
    }
    use cli::*;
    match cli.command {
        Commands::Auth(c) => auth::auth(&ctx, c).await,
        Commands::Whoami => auth::whoami(&ctx).await,
        Commands::Workspace(c) => auth::workspace(&ctx, c).await,
        Commands::Config(c) => auth::config(&ctx, c),
        Commands::Board(c) => hierarchy::board(&ctx, c).await,
        Commands::Tree => hierarchy::tree(&ctx, None).await,
        Commands::List(c) => hierarchy::list(&ctx, c).await,
        Commands::Alias(c) => hierarchy::alias(&ctx, c).await,
        Commands::Member(c) => hierarchy::member(&ctx, c).await,
        Commands::Cache(c) => hierarchy::cache(&ctx, c).await,
        Commands::Api {
            method,
            path,
            query,
            data,
        } => report::api(&ctx, method, path, query, data).await,
        Commands::Mcp(c) => mcp::run(&ctx, c).await,
    }
}

/// 0 ok, 1 generic, 2 usage, 3 auth, 4 not found, 5 rate limited.
fn exit_code(err: &anyhow::Error) -> u8 {
    match err.downcast_ref::<TrelloError>() {
        Some(TrelloError::Auth(_)) => 3,
        Some(TrelloError::NotFound(_)) | Some(TrelloError::Resolve { .. }) => 4,
        Some(TrelloError::RateLimit { .. }) => 5,
        Some(TrelloError::Invalid(_)) | Some(TrelloError::Config(_)) => 2,
        _ => 1,
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
            eprintln!("error: {}", trelloc_core::error::redact(&format!("{e:#}")));
            ExitCode::from(exit_code(&e))
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

    #[test]
    fn exit_codes() {
        let e = |c: TrelloError| exit_code(&anyhow::Error::new(c));
        assert_eq!(e(TrelloError::Auth("x".into())), 3);
        assert_eq!(e(TrelloError::NotFound("x".into())), 4);
        assert_eq!(e(TrelloError::resolve("board", "x", vec![])), 4);
        assert_eq!(
            e(TrelloError::RateLimit {
                limit: "API_TOKEN_LIMIT_EXCEEDED".into()
            }),
            5
        );
        assert_eq!(e(TrelloError::Invalid("x".into())), 2);
        assert_eq!(exit_code(&anyhow::anyhow!("other")), 1);
    }
}
