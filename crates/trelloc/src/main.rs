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
    card::CardCmd,
    collab::{AttachmentCmd, ChecklistCmd, CommentCmd, FieldCmd, LabelCmd},
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

// ponytail: parsed once per process, so the size difference is irrelevant.
#[allow(clippy::large_enum_variant)]
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
    /// List, view, create, update, move, copy, archive and delete cards
    #[command(subcommand)]
    Card(CardCmd),
    /// List, add, edit and delete comments
    #[command(subcommand)]
    Comment(CommentCmd),
    /// Checklists and their items
    #[command(subcommand)]
    Checklist(ChecklistCmd),
    /// Board labels
    #[command(subcommand)]
    Label(LabelCmd),
    /// Custom fields (needs the Custom Fields Power-Up)
    #[command(subcommand)]
    Field(FieldCmd),
    /// My cards: moved, completed, commented since; in progress, due soon, overdue
    Standup {
        #[arg(long, default_value = "yesterday")]
        since: String,
        #[arg(long)]
        board: Option<String>,
        /// Someone else's standup (username)
        #[arg(long)]
        member: Option<String>,
    },
    /// Card counts for a board: per list, per member, overdue, no due, done
    Summary {
        #[arg(long)]
        board: Option<String>,
    },
    /// Recent activity of a board, a card, or yourself
    Activity {
        #[arg(long)]
        board: Option<String>,
        #[arg(long)]
        card: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        member: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Attach files or URLs to a card
    Attach {
        card: String,
        /// Local files or http(s) URLs
        #[arg(required = true)]
        targets: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        /// Use the attachment as the card cover
        #[arg(long)]
        cover: bool,
    },
    /// List or delete attachments
    #[command(subcommand)]
    Attachment(AttachmentCmd),
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
    /// Upgrade trelloc to the latest release through the channel that installed it
    Update {
        /// Only report whether a newer release exists
        #[arg(long)]
        check: bool,
    },
}

async fn run(cli: Cli) -> Result<()> {
    let ctx = cli::Ctx {
        profile: cli.profile,
        workspace: cli.workspace,
        json: cli.json,
        yes: cli.yes,
        refresh: cli.refresh,
    };
    if let Some(profile) = &ctx.profile {
        // The MCP server path loads config itself; make the flag visible there too.
        std::env::set_var("TRELLO_PROFILE", profile);
    }
    dispatch(&ctx, cli.command).await
}

async fn dispatch(ctx: &cli::Ctx, command: Commands) -> Result<()> {
    use cli::*;
    match command {
        Commands::Auth(cmd) => auth::auth(ctx, cmd).await,
        Commands::Whoami => auth::whoami(ctx).await,
        Commands::Workspace(cmd) => auth::workspace(ctx, cmd).await,
        Commands::Config(cmd) => auth::config(ctx, cmd),
        Commands::Board(_)
        | Commands::Tree
        | Commands::List(_)
        | Commands::Alias(_)
        | Commands::Member(_)
        | Commands::Cache(_) => run_hierarchy(ctx, command).await,
        Commands::Card(cmd) => card::run(ctx, cmd).await,
        Commands::Comment(_)
        | Commands::Checklist(_)
        | Commands::Label(_)
        | Commands::Field(_)
        | Commands::Attach { .. }
        | Commands::Attachment(_) => run_collaboration(ctx, command).await,
        Commands::Standup { .. }
        | Commands::Summary { .. }
        | Commands::Activity { .. }
        | Commands::Api { .. } => run_report(ctx, command).await,
        Commands::Mcp(cmd) => mcp::run(ctx, cmd).await,
        Commands::Update { check } => update::run(ctx, check).await,
    }
}

async fn run_hierarchy(ctx: &cli::Ctx, command: Commands) -> Result<()> {
    use cli::*;
    match command {
        Commands::Board(cmd) => hierarchy::board(ctx, cmd).await,
        Commands::Tree => hierarchy::tree(ctx, None).await,
        Commands::List(cmd) => hierarchy::list(ctx, cmd).await,
        Commands::Alias(cmd) => hierarchy::alias(ctx, cmd).await,
        Commands::Member(cmd) => hierarchy::member(ctx, cmd).await,
        Commands::Cache(cmd) => hierarchy::cache(ctx, cmd).await,
        _ => unreachable!("dispatch sends only hierarchy commands"),
    }
}

async fn run_collaboration(ctx: &cli::Ctx, command: Commands) -> Result<()> {
    use cli::*;
    match command {
        Commands::Comment(cmd) => collab::comment(ctx, cmd).await,
        Commands::Checklist(cmd) => collab::checklist(ctx, cmd).await,
        Commands::Label(cmd) => collab::label(ctx, cmd).await,
        Commands::Field(cmd) => collab::field(ctx, cmd).await,
        Commands::Attach {
            card,
            targets,
            name,
            cover,
        } => collab::attach(ctx, card, targets, name, cover).await,
        Commands::Attachment(cmd) => collab::attachment(ctx, cmd).await,
        _ => unreachable!("dispatch sends only collaboration commands"),
    }
}

async fn run_report(ctx: &cli::Ctx, command: Commands) -> Result<()> {
    use cli::*;
    match command {
        Commands::Standup {
            since,
            board,
            member,
        } => report::standup(ctx, since, board, member).await,
        Commands::Summary { board } => report::summary(ctx, board).await,
        Commands::Activity {
            board,
            card,
            since,
            member,
            limit,
        } => {
            report::activity(
                ctx,
                trelloc_core::ops::ActivityInput {
                    board,
                    card,
                    since,
                    member,
                    limit: Some(limit),
                },
            )
            .await
        }
        Commands::Api {
            method,
            path,
            query,
            data,
        } => report::api(ctx, method, path, query, data).await,
        _ => unreachable!("dispatch sends only report commands"),
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
