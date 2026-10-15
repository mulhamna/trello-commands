//! auth, whoami, workspace, config.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::json;
use trelloc_core::{
    config::{mask_secret, ConfigFile, Profile, TrelloConfig},
    model::Member,
    TrelloClient,
};

use super::Ctx;
use crate::output::{is_tty, print_json, print_table};

#[derive(Debug, Subcommand)]
pub enum AuthCmd {
    /// Store an API key and token, then cache who you are
    #[command(long_about = crate::help_text::AUTH_LOGIN_LONG_ABOUT)]
    Login {
        /// API key (trello.com/power-ups/admin → API key). Prompted when
        /// omitted (falls back to TRELLO_API_KEY without a terminal).
        #[arg(long)]
        api_key: Option<String>,
        /// User token, or `-` to read it from stdin. When omitted, trelloc
        /// opens the authorize page and prompts for it.
        #[arg(long)]
        token: Option<String>,
    },
    /// Show profile, member, credentials (masked) and rate-limit headroom
    Status,
    /// Remove the stored key and token from the profile
    Logout,
    /// Manage profiles
    #[command(subcommand)]
    Profile(ProfileCmd),
}

#[derive(Debug, Subcommand)]
pub enum ProfileCmd {
    /// List saved profiles
    List,
    /// Make a profile the default (picker when omitted)
    Use { name: Option<String> },
    /// Delete a profile
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCmd {
    /// List workspaces the token can see
    List,
    /// Set the default workspace for board-name lookups (picker when omitted)
    Use { workspace: Option<String> },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print a setting (or all settings of the profile)
    Get { key: Option<String> },
    /// Set default_board, default_list, workspace, timezone,
    /// default_card_limit or timeout_secs (`none` clears)
    Set { key: String, value: String },
}

fn profile_name(ctx: &Ctx, file: &ConfigFile) -> String {
    ctx.profile
        .clone()
        .unwrap_or_else(|| file.selected_profile_name())
}

pub async fn auth(ctx: &Ctx, cmd: AuthCmd) -> Result<()> {
    match cmd {
        AuthCmd::Login { api_key, token } => login(ctx, api_key, token, true).await,
        AuthCmd::Status => status(ctx).await,
        AuthCmd::Logout => {
            let mut file = ConfigFile::load()?;
            let name = profile_name(ctx, &file);
            let p = file.profile_mut(&name);
            p.api_key = None;
            p.token = None;
            file.save()?;
            println!("Removed the API key and token from profile '{name}'.");
            Ok(())
        }
        AuthCmd::Profile(p) => profile(ctx, p),
    }
}

/// `https://trello.com/1/authorize?…` for `key`.
pub fn authorize_url(key: &str) -> String {
    format!(
        "https://trello.com/1/authorize?expiration=never&scope=read,write,account&response_type=token&name=trelloc&key={key}"
    )
}

fn secret_arg(value: Option<String>, prompt: &str, help: &str, env: &str) -> Result<String> {
    let v = match value.as_deref() {
        Some("-") => {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
            buf
        }
        Some(v) => v.to_string(),
        // Masked (not hidden) so a paste visibly lands.
        None if is_tty() => inquire::Password::new(prompt)
            .with_display_mode(inquire::PasswordDisplayMode::Masked)
            .without_confirmation()
            .with_help_message(help)
            .prompt()?,
        None => match std::env::var(env) {
            Ok(v) if !v.trim().is_empty() => v,
            _ => bail!("missing {prompt} pass it as a flag, `-` for stdin, or set {env}"),
        },
    };
    let v = v.trim().to_string();
    if v.is_empty() {
        bail!("{prompt} is empty");
    }
    Ok(v)
}

/// Interactive-aware login. `offer_mcp` asks to continue into
/// `mcp install` afterwards (off when called from `mcp install` itself).
pub async fn login(
    ctx: &Ctx,
    api_key: Option<String>,
    token: Option<String>,
    offer_mcp: bool,
) -> Result<()> {
    let mut file = ConfigFile::load()?;
    let name = profile_name(ctx, &file);

    if api_key.is_none() && is_tty() {
        eprintln!(
            "Get an API key at https://trello.com/power-ups/admin → New → API key → Generate."
        );
    }
    let key = secret_arg(
        api_key,
        "Trello API key:",
        "Paste with Cmd+V / Ctrl+Shift+V; input shows as ****",
        "TRELLO_API_KEY",
    )?;
    if token.is_none() && is_tty() {
        let url = authorize_url(&key);
        eprintln!("Opening the Trello authorize page. Click Allow, then paste the token:\n  {url}");
        // ponytail: a failed browser launch is fine, the URL is printed above.
        let _ = open::that(&url);
    }
    let token = secret_arg(
        token,
        "Trello token:",
        "Shown after you click Allow on the authorize page",
        "TRELLO_TOKEN",
    )?;

    let base = ctx.config().map(|c| c.base_url).unwrap_or_default();
    let client = TrelloClient::new(&TrelloConfig::with_credentials(
        key.clone(),
        token.clone(),
        base,
    ))?;
    let spinner = crate::output::spinner("Verifying with Trello…");
    let me = client.me().await;
    spinner.finish_and_clear();
    let me = me.context("Trello rejected the key/token")?;

    let p = file.profile_mut(&name);
    p.api_key = Some(key);
    p.token = Some(token);
    cache_member(p, &me);
    if file.current_profile.is_none() {
        file.current_profile = Some(name.clone());
    }
    let path = file.save()?;
    println!(
        "Logged in as {} (@{}) — saved to profile '{name}' ({}).",
        me.full_name,
        me.username,
        path.display()
    );
    println!("Next: `trelloc tree`, then `trelloc board use <board>` to set a default board.");
    if offer_mcp
        && is_tty()
        && inquire::Confirm::new("Set up the MCP server for your AI clients now?")
            .with_default(true)
            .with_help_message("Registers `trelloc mcp serve` in Claude Code, Cursor, Codex, …")
            .prompt()?
    {
        println!();
        super::mcp::interactive_install(ctx).await?;
    }
    Ok(())
}

fn cache_member(p: &mut Profile, me: &Member) {
    p.member_id = Some(me.id.clone());
    p.username = Some(me.username.clone());
    p.full_name = Some(me.full_name.clone());
}

async fn status(ctx: &Ctx) -> Result<()> {
    let cfg = ctx.config()?;
    // One cheap nested call (not members-limited) to verify the credentials
    // and read the rate-limit headers.
    let (verified, remaining) = match TrelloClient::new(&cfg) {
        Ok(c) => {
            let ok = c.my_workspaces().await.is_ok();
            (Some(ok), c.limiter().remaining())
        }
        Err(_) => (None, (None, None)),
    };
    let member = cfg.member.as_ref();
    let out = json!({
        "profile": cfg.profile,
        "member": member.map(|m| json!({ "id": m.id, "username": m.username, "full_name": m.full_name })),
        "api_key": cfg.masked_key(),
        "token": cfg.masked_token(),
        "verified": verified,
        "rate_limit": { "token_remaining": remaining.0, "key_remaining": remaining.1 },
        "default_board": cfg.default_board,
        "workspace": cfg.workspace,
        "config_path": cfg.config_path.display().to_string(),
        "timezone": cfg.timezone,
    });
    if ctx.json {
        return print_json(&out);
    }
    let show = |v: Option<String>| v.unwrap_or_else(|| "(missing)".into());
    println!("Profile:    {}", cfg.profile);
    println!(
        "Member:     {}",
        member
            .map(|m| format!(
                "{} (@{})",
                m.full_name.clone().unwrap_or_default(),
                m.username.clone().unwrap_or_default()
            ))
            .unwrap_or_else(|| "(not cached; run `trelloc whoami --refresh`)".into())
    );
    println!("API key:    {}", show(cfg.masked_key()));
    println!("Token:      {}", show(cfg.masked_token()));
    println!(
        "Verified:   {}",
        match verified {
            Some(true) => "yes",
            Some(false) => "no (request failed)",
            None => "no credentials",
        }
    );
    if let (Some(t), k) = remaining {
        println!(
            "Rate limit: {t} requests left in this 10 s window (token){}",
            k.map(|k| format!(", {k} (key)")).unwrap_or_default()
        );
    }
    if let Some(b) = &cfg.default_board {
        println!("Board:      {b}");
    }
    println!("Config:     {}", cfg.config_path.display());
    Ok(())
}

fn profile_label(name: &str, p: &Profile) -> String {
    format!(
        "{name}  (@{} · {})",
        p.username.as_deref().unwrap_or("?"),
        p.token
            .as_deref()
            .map(mask_secret)
            .unwrap_or_else(|| "no token".into())
    )
}

fn profile(ctx: &Ctx, cmd: ProfileCmd) -> Result<()> {
    let mut file = ConfigFile::load()?;
    match cmd {
        ProfileCmd::List => {
            let current = file.selected_profile_name();
            if ctx.json {
                return print_json(&file.profiles.keys().collect::<Vec<_>>());
            }
            print_table(
                &["", "Profile", "Member", "Default board", "Token"],
                file.profiles.iter().map(|(name, p)| {
                    vec![
                        if *name == current { "*" } else { "" }.to_string(),
                        name.clone(),
                        p.username.clone().unwrap_or_default(),
                        p.default_board.clone().unwrap_or_default(),
                        p.token.as_deref().map(mask_secret).unwrap_or_default(),
                    ]
                }),
            );
        }
        ProfileCmd::Use { name } => {
            let name = match name {
                Some(n) => n,
                None => {
                    crate::output::require_tty("profile name", "<name>")?;
                    if file.profiles.is_empty() {
                        bail!("no profiles yet; run `trelloc auth login`");
                    }
                    let names: Vec<String> = file.profiles.keys().cloned().collect();
                    let labels: Vec<String> = file
                        .profiles
                        .iter()
                        .map(|(n, p)| profile_label(n, p))
                        .collect();
                    let pick =
                        inquire::Select::new("Use which profile?", labels.clone()).prompt()?;
                    names[labels.iter().position(|l| *l == pick).unwrap_or(0)].clone()
                }
            };
            if !file.profiles.contains_key(&name) {
                bail!("no profile named '{name}'");
            }
            file.current_profile = Some(name.clone());
            file.save()?;
            println!("Using profile '{name}'.");
        }
        ProfileCmd::Remove { name } => {
            if file.profiles.remove(&name).is_none() {
                bail!("no profile named '{name}'");
            }
            if file.current_profile.as_deref() == Some(name.as_str()) {
                file.current_profile = None;
            }
            file.save()?;
            println!("Removed profile '{name}'.");
        }
    }
    Ok(())
}

/// From the profile cache; `--refresh` calls `/1/members/me` and re-caches.
pub async fn whoami(ctx: &Ctx) -> Result<()> {
    let cfg = ctx.config()?;
    let member = if ctx.refresh {
        let me = TrelloClient::new(&cfg)?.me().await?;
        let mut file = ConfigFile::load()?;
        // Only cache for the profile's own token, not an env override.
        if std::env::var("TRELLO_TOKEN").map_or(true, |t| t.trim().is_empty()) {
            cache_member(file.profile_mut(&cfg.profile), &me);
            file.save()?;
        }
        json!({ "id": me.id, "username": me.username, "full_name": me.full_name })
    } else {
        let m = cfg.member()?;
        json!({ "id": m.id, "username": m.username, "full_name": m.full_name })
    };
    if ctx.json {
        return print_json(&member);
    }
    println!(
        "{} (@{}) — {}",
        member["full_name"].as_str().unwrap_or_default(),
        member["username"].as_str().unwrap_or_default(),
        member["id"].as_str().unwrap_or_default()
    );
    Ok(())
}

pub async fn workspace(ctx: &Ctx, cmd: WorkspaceCmd) -> Result<()> {
    let s = ctx.session()?;
    let list = s.client.my_workspaces().await?;
    match cmd {
        WorkspaceCmd::List => {
            if ctx.json {
                return print_json(&list);
            }
            let current = s.cfg.workspace.clone().unwrap_or_default();
            print_table(
                &["", "ID", "Name", "Short name"],
                list.iter().map(|w| {
                    vec![
                        if w.id == current || w.name == current {
                            "*"
                        } else {
                            ""
                        }
                        .to_string(),
                        w.id.clone(),
                        w.display_name.clone(),
                        w.name.clone(),
                    ]
                }),
            );
        }
        WorkspaceCmd::Use { workspace } => {
            let w = match workspace {
                Some(t) => list
                    .iter()
                    .find(|x| {
                        x.id == t
                            || x.name.eq_ignore_ascii_case(&t)
                            || x.display_name.eq_ignore_ascii_case(&t)
                    })
                    .with_context(|| format!("workspace '{t}' not found"))?,
                None => {
                    crate::output::require_tty("workspace", "<workspace>")?;
                    let labels: Vec<String> = list
                        .iter()
                        .map(|w| format!("{} ({})", w.display_name, w.name))
                        .collect();
                    let pick =
                        inquire::Select::new("Use which workspace?", labels.clone()).prompt()?;
                    &list[labels.iter().position(|l| *l == pick).unwrap_or(0)]
                }
            };
            let mut file = ConfigFile::load()?;
            let name = profile_name(ctx, &file);
            file.profile_mut(&name).workspace = Some(w.id.clone());
            file.save()?;
            println!(
                "Profile '{name}' now resolves board names in workspace '{}'.",
                w.display_name
            );
        }
    }
    Ok(())
}

const KEYS: [&str; 6] = [
    "default_board",
    "default_list",
    "workspace",
    "timezone",
    "default_card_limit",
    "timeout_secs",
];

pub fn config(ctx: &Ctx, cmd: ConfigCmd) -> Result<()> {
    let mut file = ConfigFile::load()?;
    let name = profile_name(ctx, &file);
    match cmd {
        ConfigCmd::Get { key } => {
            let p = file.profiles.get(&name).cloned().unwrap_or_default();
            let all = json!({
                "default_board": p.default_board,
                "default_list": p.default_list,
                "workspace": p.workspace,
                "timezone": p.timezone,
                "default_card_limit": p.default_card_limit,
                "timeout_secs": p.timeout_secs,
            });
            match key {
                Some(k) => {
                    let v = all.get(&k).with_context(|| {
                        format!("unknown key '{k}' (one of {})", KEYS.join(", "))
                    })?;
                    if ctx.json {
                        print_json(v)?;
                    } else if !v.is_null() {
                        println!(
                            "{}",
                            v.as_str().map(str::to_string).unwrap_or(v.to_string())
                        );
                    }
                }
                None => print_json(&all)?,
            }
        }
        ConfigCmd::Set { key, value } => {
            let p = file.profile_mut(&name);
            let clear = value.eq_ignore_ascii_case("none") || value.is_empty();
            let text = (!clear).then(|| value.clone());
            match key.as_str() {
                "default_board" => p.default_board = text,
                "default_list" => p.default_list = text,
                "workspace" => p.workspace = text,
                "timezone" => {
                    if !clear && !trelloc_core::time::is_valid_tz(&value) {
                        bail!("unknown timezone '{value}' (use an IANA name like Asia/Jakarta)");
                    }
                    p.timezone = text
                }
                "timeout_secs" => p.timeout_secs = if clear { None } else { Some(value.parse()?) },
                "default_card_limit" => {
                    p.default_card_limit = if clear { None } else { Some(value.parse()?) }
                }
                other => bail!("unknown key '{other}' (one of {})", KEYS.join(", ")),
            }
            file.save()?;
            println!("Set {key} for profile '{name}'.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_url_asks_for_a_non_expiring_read_write_token() {
        let u = authorize_url("k");
        for part in [
            "expiration=never",
            "scope=read,write,account",
            "response_type=token",
            "name=trelloc",
            "key=k",
        ] {
            assert!(u.contains(part), "{u}");
        }
    }
}
