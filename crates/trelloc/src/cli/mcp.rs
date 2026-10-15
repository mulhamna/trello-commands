//! `trelloc mcp serve|install|doctor`. Registration goes through kurir; the
//! registered entry runs this same binary with `mcp serve` and never
//! carries the API key or token.

use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::{Subcommand, ValueEnum};
use trelloc_core::config::{mask_secret, ConfigFile, TrelloConfig};

use super::Ctx;
use kurir::{doctor as kurir_doctor, register, Harness, RegistrationOptions, Scope, ServerSpec};
use trelloc_core::TrelloClient;

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
    /// Register `trelloc mcp serve` in MCP clients' configs.
    ///
    /// Omit `--client` to pick clients and profiles interactively.
    Install {
        /// Client to register with; repeat for several
        #[arg(long = "client", value_enum)]
        clients: Vec<McpClient>,
        /// Server entry name. Defaults to `trelloc` so it never collides
        /// with Trello's official server, which clients usually call `trello`.
        #[arg(long, default_value = "trelloc")]
        name: String,
        /// Binary to register (default: trelloc on PATH)
        #[arg(long, default_value = "trelloc")]
        command: String,
        /// Pin a trelloc profile via TRELLO_PROFILE; repeat for several
        /// (each gets its own `<name>-<profile>` entry). Defaults to the
        /// global --profile when given.
        #[arg(long = "server-profile")]
        server_profiles: Vec<String>,
        /// Print what would be written
        #[arg(long)]
        print: bool,
        #[arg(long)]
        dry_run: bool,
        /// Overwrite an existing entry
        #[arg(long)]
        force: bool,
    },
    /// Check binary, credentials and client config readiness
    Doctor {
        #[arg(long, value_enum)]
        client: Option<McpClient>,
        #[arg(long, default_value = "trelloc")]
        command: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Transport {
    Stdio,
    StreamableHttp,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum McpClient {
    ClaudeCode,
    #[value(name = "claude-code-cli")]
    ClaudeCodeCli,
    ClaudeDesktop,
    Cursor,
    GeminiCli,
    Codex,
    #[value(name = "vscode")]
    Vscode,
    #[value(name = "copilot-cli")]
    CopilotCli,
    #[value(name = "opencode")]
    OpenCode,
    Windsurf,
    Zed,
    #[value(name = "openclaw")]
    OpenClaw,
    Hermes,
    #[value(name = "antigravity-cli", alias = "antigravity")]
    AntigravityCli,
    AntigravityDesktop,
    #[value(name = "generic-json", alias = "omp")]
    GenericJson,
    #[value(name = "arsy-code")]
    ArsyCode,
}

const ALL_CLIENTS: [McpClient; 17] = [
    McpClient::ClaudeCode,
    McpClient::ClaudeCodeCli,
    McpClient::ClaudeDesktop,
    McpClient::Cursor,
    McpClient::GeminiCli,
    McpClient::Codex,
    McpClient::Vscode,
    McpClient::CopilotCli,
    McpClient::OpenCode,
    McpClient::Windsurf,
    McpClient::Zed,
    McpClient::OpenClaw,
    McpClient::Hermes,
    McpClient::AntigravityCli,
    McpClient::AntigravityDesktop,
    McpClient::GenericJson,
    McpClient::ArsyCode,
];

impl McpClient {
    fn harness(self) -> Harness {
        const HARNESSES: [Harness; 17] = [
            Harness::ClaudeCode,
            Harness::ClaudeCodeCli,
            Harness::ClaudeDesktop,
            Harness::Cursor,
            Harness::GeminiCli,
            Harness::Codex,
            Harness::Vscode,
            Harness::CopilotCli,
            Harness::OpenCode,
            Harness::Windsurf,
            Harness::Zed,
            Harness::OpenClaw,
            Harness::Hermes,
            Harness::AntigravityCli,
            Harness::AntigravityDesktop,
            Harness::Omp,
            Harness::ArsyCode,
        ];
        HARNESSES[self as usize]
    }
}

impl std::fmt::Display for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const LABELS: [&str; 17] = [
            "claude-code         (~/.claude.json)",
            "claude-code-cli     (delegates to `claude mcp add`)",
            "claude-desktop      (Claude Desktop config)",
            "cursor              (Cursor mcp.json)",
            "gemini-cli          (delegates to `gemini mcp add`)",
            "codex               (delegates to `codex mcp add`)",
            "vscode              (delegates to `code --add-mcp`)",
            "copilot-cli         (~/.copilot/mcp-config.json)",
            "opencode            (opencode.json)",
            "windsurf            (Windsurf config)",
            "zed                 (Zed settings)",
            "openclaw            (openclaw.json)",
            "hermes              (delegates to `hermes mcp add`)",
            "antigravity-cli     (Antigravity CLI config)",
            "antigravity-desktop (Antigravity Desktop config)",
            "generic-json        (print snippet only)",
            "arsy-code           (delegates to `arsy mcp add`)",
        ];
        f.write_str(LABELS[*self as usize])
    }
}

pub async fn run(ctx: &Ctx, cmd: McpCmd) -> Result<()> {
    match cmd {
        McpCmd::Serve {
            transport,
            host,
            port,
            path,
        } => match transport {
            Transport::Stdio => trelloc_mcp::run_stdio().await,
            Transport::StreamableHttp => {
                let path = if path.starts_with('/') {
                    path
                } else {
                    format!("/{path}")
                };
                trelloc_mcp::run_streamable_http(&host, port, &path).await
            }
        },
        McpCmd::Install {
            clients,
            name,
            command,
            server_profiles,
            print,
            dry_run,
            force,
        } => {
            let server_profiles = if server_profiles.is_empty() {
                ctx.profile.iter().cloned().collect()
            } else {
                server_profiles
            };
            if clients.is_empty() {
                let plan = interactive_setup(ctx, &command).await?;
                install_all(
                    &plan.clients,
                    &plan.profiles,
                    &name,
                    &plan.command,
                    print,
                    dry_run,
                    force,
                )
            } else {
                install_all(
                    &clients,
                    &server_profiles,
                    &name,
                    &command,
                    print,
                    dry_run,
                    force,
                )
            }
        }
        McpCmd::Doctor { client, command } => doctor(client, &command).await,
    }
}

/// What the interactive flow decided to install.
pub struct InstallPlan {
    pub clients: Vec<McpClient>,
    pub profiles: Vec<String>,
    pub command: String,
}

fn has_credentials() -> bool {
    TrelloConfig::load()
        .map(|c| c.credentials().is_ok())
        .unwrap_or(false)
}

/// Banner, prerequisite checklist (logging in on the spot if needed), then
/// multi-select for clients and — with several profiles — for profiles.
pub async fn interactive_setup(ctx: &Ctx, command: &str) -> Result<InstallPlan> {
    crate::output::require_tty("MCP client", "--client <target>")?;
    println!("{}", crate::help_text::BANNER.trim_end());
    println!();
    println!("trelloc mcp install — interactive setup");
    println!("────────────────────────────────────────");

    // Binary: prefer what's on PATH, else this very executable.
    let command = match resolve_command_path(command) {
        Some(p) => {
            println!("[ok]   trelloc binary:  {}", p.display());
            p.display().to_string()
        }
        None => {
            let exe = env::current_exe().context("cannot locate the trelloc executable")?;
            println!(
                "[warn] '{command}' is not on PATH; registering {}",
                exe.display()
            );
            exe.display().to_string()
        }
    };

    if !has_credentials() {
        println!("[fail] Trello auth:      no API key / token configured");
        println!();
        println!("The MCP server needs Trello credentials first — starting `trelloc auth login`.");
        println!();
        Box::pin(super::auth::login(ctx, None, None, false)).await?;
        println!();
    }
    let file = ConfigFile::load()?;
    let names: Vec<String> = file
        .profiles
        .iter()
        .filter(|(_, p)| p.api_key.is_some() && p.token.is_some())
        .map(|(n, _)| n.clone())
        .collect();
    if names.is_empty() {
        bail!("no usable profile; run `trelloc auth login` and try again");
    }
    println!(
        "[ok]   Trello auth:     {} profile(s): {}",
        names.len(),
        names.join(", ")
    );
    println!();

    let clients = inquire::MultiSelect::new(
        "Install into which MCP clients? (space to toggle, enter to confirm)",
        ALL_CLIENTS.to_vec(),
    )
    .with_page_size(17)
    .with_validator(|picked: &[inquire::list_option::ListOption<&McpClient>]| {
        Ok(if picked.is_empty() {
            inquire::validator::Validation::Invalid("pick at least one client".into())
        } else {
            inquire::validator::Validation::Valid
        })
    })
    .prompt()
    .context("MCP client selection cancelled")?;

    let profiles = if names.len() == 1 {
        names
    } else {
        let current = file.selected_profile_name();
        let labels: Vec<String> = names
            .iter()
            .map(|n| {
                let p = &file.profiles[n];
                format!(
                    "{n}  (@{} · {})",
                    p.username.as_deref().unwrap_or("?"),
                    p.token.as_deref().map(mask_secret).unwrap_or_default()
                )
            })
            .collect();
        let default: Vec<usize> = names
            .iter()
            .position(|n| *n == current)
            .into_iter()
            .collect();
        let picked = inquire::MultiSelect::new(
            "Which profiles? Each becomes its own server entry (space to toggle)",
            labels.clone(),
        )
        .with_default(&default)
        .with_validator(|picked: &[inquire::list_option::ListOption<&String>]| {
            Ok(if picked.is_empty() {
                inquire::validator::Validation::Invalid("pick at least one profile".into())
            } else {
                inquire::validator::Validation::Valid
            })
        })
        .prompt()
        .context("profile selection cancelled")?;
        picked
            .iter()
            .filter_map(|l| labels.iter().position(|x| x == l).map(|i| names[i].clone()))
            .collect()
    };
    Ok(InstallPlan {
        clients,
        profiles,
        command,
    })
}

/// Snippet meant to be pasted as-is. kurir's own snippet redacts every env
/// value; ours only ever holds TRELLO_PROFILE (a profile name, not a
/// secret), so print it verbatim or the pasted entry would be broken.
pub fn copyable_snippet(spec: &ServerSpec) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "command": spec.command,
        "args": spec.args,
    });
    if !spec.env.is_empty() {
        entry["env"] = serde_json::json!(spec.env);
    }
    serde_json::json!({ "mcpServers": { spec.name.clone(): entry } })
}

/// Install an interactive plan with the default entry name.
pub fn install_plan(plan: &InstallPlan) -> Result<()> {
    install_all(
        &plan.clients,
        &plan.profiles,
        DEFAULT_ENTRY_NAME,
        &plan.command,
        false,
        false,
        false,
    )
}

pub const DEFAULT_ENTRY_NAME: &str = "trelloc";

/// Entry name: the base name for zero/one profile, `<base>-<profile>` for several.
pub fn entry_name(base: &str, profile: Option<&str>, profile_count: usize) -> String {
    match profile {
        Some(p) if profile_count > 1 => format!("{base}-{p}"),
        _ => base.to_string(),
    }
}

fn install_all(
    clients: &[McpClient],
    profiles: &[String],
    name: &str,
    command: &str,
    print: bool,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let targets: Vec<Option<&str>> = if profiles.is_empty() {
        vec![None]
    } else {
        profiles.iter().map(|p| Some(p.as_str())).collect()
    };
    let mut failed = 0;
    for client in clients {
        for profile in &targets {
            let entry = entry_name(name, *profile, profiles.len());
            if let Err(e) = install(*client, &entry, command, *profile, print, dry_run, force) {
                failed += 1;
                eprintln!("[fail] {entry} → {client}: {e:#}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} MCP registration(s) failed");
    }
    Ok(())
}

pub fn server_spec(name: &str, command: String, profile: Option<&str>) -> ServerSpec {
    let mut spec = ServerSpec::stdio(name, command, vec!["mcp".into(), "serve".into()]);
    if let Some(p) = profile {
        spec.env.insert("TRELLO_PROFILE".into(), p.to_string());
    }
    spec
}

fn install(
    client: McpClient,
    name: &str,
    command: &str,
    profile: Option<&str>,
    print: bool,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let harness = client.harness();
    let resolved = resolve_command_for_client(client, command, dry_run)?;
    let spec = server_spec(name, resolved, profile);

    if harness.is_snippet_only() {
        println!(
            "{}",
            serde_json::to_string_pretty(&copyable_snippet(&spec))?
        );
        return Ok(());
    }
    let options = RegistrationOptions {
        scope: Scope::User,
        force,
        dry_run,
        print,
        ..RegistrationOptions::default()
    };
    let result = register(harness, &spec, &options).context("MCP registration failed")?;
    if result.changed {
        let pinned = profile
            .map(|p| format!(" (profile {p})"))
            .unwrap_or_default();
        println!("[ok]   Installed '{name}'{pinned} for {harness}.");
    } else if result.action == "already-configured" {
        println!("MCP entry '{name}' is already configured (use --force to overwrite).");
    } else if dry_run {
        println!("Dry run: no client config written.");
    }
    Ok(())
}

async fn doctor(client: Option<McpClient>, command: &str) -> Result<()> {
    println!("trelloc mcp doctor");
    match resolve_command_path(command) {
        Some(p) => println!("[ok]   binary on PATH: {}", p.display()),
        None => println!("[warn] '{command}' not on PATH; MCP clients won't be able to start it"),
    }
    match TrelloConfig::load() {
        Ok(c) if c.credentials().is_err() => {
            println!("[warn] no API key / token; run `trelloc auth login`")
        }
        Ok(c) => {
            println!(
                "[ok]   profile '{}' (key {}, token {})",
                c.profile,
                c.masked_key().unwrap_or_default(),
                c.masked_token().unwrap_or_default()
            );
            if c.member.is_none() {
                println!("[warn] member not cached; run `trelloc whoami --refresh`");
            }
            // One cheap nested call: proves the credentials and reads headroom.
            match TrelloClient::new(&c) {
                Ok(client) => match client.my_workspaces().await {
                    Ok(_) => {
                        let (t, k) = client.limiter().remaining();
                        println!(
                            "[ok]   Trello API reachable; rate limit left: {} (token), {} (key) per 10 s",
                            t.map_or("?".into(), |v| v.to_string()),
                            k.map_or("?".into(), |v| v.to_string())
                        );
                    }
                    Err(e) => println!(
                        "[fail] test call failed: {}",
                        trelloc_core::error::redact(&e.to_string())
                    ),
                },
                Err(e) => println!("[fail] client: {e}"),
            }
        }
        Err(e) => println!("[fail] config: {e}"),
    }
    let reports = kurir_doctor(
        client.map(McpClient::harness),
        &RegistrationOptions::default(),
    )
    .context("MCP client check failed")?;
    let mut blocking = 0;
    for report in reports {
        let status = if report.available {
            "ok  "
        } else {
            blocking += 1;
            "warn"
        };
        let target = report
            .target
            .map(|p| format!(" ({})", p.display()))
            .unwrap_or_default();
        println!("[{status}] {}{target}: {}", report.harness, report.detail);
    }
    if client.is_some() && blocking > 0 {
        bail!("{blocking} blocking issue(s) found");
    }
    Ok(())
}

fn resolve_command_for_client(client: McpClient, command: &str, dry_run: bool) -> Result<String> {
    if client.harness().is_snippet_only() {
        return Ok(command.to_string());
    }
    if let Some(p) = resolve_command_path(command) {
        return Ok(p.display().to_string());
    }
    if dry_run {
        eprintln!("[warn] '{command}' was not found on PATH.");
        return Ok(command.to_string());
    }
    bail!(
        "'{command}' was not found on PATH. Install trelloc first (npm i -g @mulham28/trelloc, cargo install trelloc) or pass --command /path/to/trelloc"
    )
}

fn resolve_command_path(command: &str) -> Option<PathBuf> {
    let path = PathBuf::from(command);
    if path.components().count() > 1 || path.is_absolute() {
        return executable_candidates(&path)
            .into_iter()
            .find(|c| c.is_file());
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|dir| {
            executable_candidates(&dir.join(command))
                .into_iter()
                .find(|c| c.is_file())
        })
    })
}

#[cfg(target_os = "windows")]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    if path.extension().is_some() {
        return out;
    }
    let exts = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    for ext in exts.split(';').map(|e| e.trim().trim_start_matches('.')) {
        if !ext.is_empty() {
            out.push(path.with_extension(ext));
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    vec![path.to_path_buf()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurir::registration::snippet_for;

    #[test]
    fn spec_runs_mcp_serve_without_token() {
        let spec = server_spec("trelloc", "/usr/local/bin/trelloc".into(), Some("work"));
        assert_eq!(spec.command.as_deref(), Some("/usr/local/bin/trelloc"));
        assert_eq!(spec.args, vec!["mcp", "serve"]);
        assert_eq!(spec.transport, kurir::Transport::Stdio);
        assert_eq!(
            spec.env.get("TRELLO_PROFILE").map(String::as_str),
            Some("work")
        );
        let json = serde_json::to_string(&snippet_for(&spec)).unwrap();
        assert!(!json.contains("TRELLO_TOKEN"));
        assert!(!json.contains("TRELLO_API_KEY"));
        assert!(!json.contains("oauth"));
        let pasted = copyable_snippet(&spec);
        assert_eq!(
            pasted["mcpServers"]["trelloc"]["env"]["TRELLO_PROFILE"],
            serde_json::json!("work")
        );
    }

    #[test]
    fn default_entry_name_does_not_clash_with_official_server() {
        use clap::Parser;
        #[derive(Parser)]
        struct T {
            #[command(subcommand)]
            cmd: McpCmd,
        }
        let t = T::try_parse_from(["t", "install", "--client", "generic-json"]).unwrap();
        match t.cmd {
            McpCmd::Install { name, .. } => assert_eq!(name, "trelloc"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn clients_map_to_harnesses() {
        assert_eq!(McpClient::ClaudeCode.harness(), Harness::ClaudeCode);
        assert_eq!(McpClient::Codex.harness(), Harness::Codex);
        assert_eq!(McpClient::GenericJson.harness(), Harness::Omp);
        assert_eq!(McpClient::ArsyCode.harness(), Harness::ArsyCode);
        for (i, c) in ALL_CLIENTS.iter().enumerate() {
            assert_eq!(*c as usize, i);
        }
    }

    #[test]
    fn missing_binary_is_an_error_unless_dry_run() {
        let err = resolve_command_for_client(McpClient::Cursor, "definitely-not-trelloc", false)
            .unwrap_err();
        assert!(err.to_string().contains("not found on PATH"));
        assert!(
            resolve_command_for_client(McpClient::Cursor, "definitely-not-trelloc", true).is_ok()
        );
    }
}
