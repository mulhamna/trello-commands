//! `trelloc update`: check the latest GitHub release and upgrade through the
//! channel that installed this binary.

use std::{path::Path, process::Command, time::Duration};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use super::Ctx;
use crate::output::{confirm, print_json};

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const LATEST_RELEASE: &str =
    "https://api.github.com/repos/mulhamna/trello-commands/releases/latest";
const INSTALL_SH: &str =
    "https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.sh";
const INSTALL_PS1: &str =
    "https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.ps1";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

pub async fn run(ctx: &Ctx, check: bool) -> Result<()> {
    let release: Release = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .get(LATEST_RELEASE)
        .header(reqwest::header::USER_AGENT, format!("trelloc/{CURRENT}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .context("checking the latest release on GitHub")?
        .json()
        .await
        .context("reading the latest release from GitHub")?;
    let latest = release.tag_name.trim_start_matches('v');
    let available = is_newer(latest, CURRENT);
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("locating the trelloc binary")?;
    let cmd = upgrade_command(&exe);
    // `sh -c "<script>"` reads better as just the script.
    let shown = match cmd.as_slice() {
        [_, flag, script] if flag == "-c" => script.clone(),
        _ => cmd.join(" "),
    };

    if ctx.json {
        return print_json(&json!({
            "current": CURRENT,
            "latest": latest,
            "update_available": available,
            "url": release.html_url,
            "command": shown,
        }));
    }
    if !available {
        println!("trelloc {CURRENT} is up to date");
        return Ok(());
    }
    println!(
        "update available: trelloc {CURRENT} -> {latest} ({})",
        release.html_url
    );
    // ponytail: Windows locks a running .exe, so print the command instead of running it.
    if check || cfg!(windows) {
        println!("run: {shown}");
        return Ok(());
    }
    if !confirm(ctx.yes, &format!("Run `{shown}`?"))? {
        return Ok(());
    }
    let mut child = Command::new(&cmd[0]);
    child.args(&cmd[1..]);
    if cmd[0] == "sh" {
        // The install script defaults to ~/.local/bin; keep the binary where it is.
        if let Some(dir) = exe.parent() {
            child.env("TRELLOC_INSTALL_DIR", dir);
        }
    }
    let status = child
        .status()
        .with_context(|| format!("running {}", cmd[0]))?;
    if !status.success() {
        bail!("`{shown}` failed ({status})");
    }
    Ok(())
}

/// The upgrade command for the channel that installed `exe`, guessed from its path.
fn upgrade_command(exe: &Path) -> Vec<String> {
    let p = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    let cmd: &[&str] = if p.contains("/node_modules/") {
        &["npm", "install", "-g", "@mulham28/trelloc@latest"]
    } else if p.contains("/cellar/") || p.contains("/homebrew/") || p.contains("/linuxbrew/") {
        &["brew", "upgrade", "mulhamna/tap/trelloc"]
    } else if p.contains("/scoop/") {
        &["scoop", "update", "trelloc"]
    } else if p.contains("/.cargo/bin/") {
        &["cargo", "install", "trelloc", "--locked", "--force"]
    } else if cfg!(windows) {
        return vec![
            "powershell".into(),
            "-c".into(),
            format!("irm {INSTALL_PS1} | iex"),
        ];
    } else {
        return vec![
            "sh".into(),
            "-c".into(),
            format!("curl -fsSL {INSTALL_SH} | bash"),
        ];
    };
    cmd.iter().map(|s| s.to_string()).collect()
}

/// Numeric `x.y.z` comparison; non-numeric parts count as 0.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|n| n.parse().unwrap_or(0)).collect() };
    parse(latest) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("1.1.1", "1.1.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.1.0", "1.1.0"));
        assert!(!is_newer("1.0.9", "1.1.0"));
    }

    #[test]
    fn picks_the_installing_channel() {
        let first = |p: &str| upgrade_command(Path::new(p))[0].clone();
        assert_eq!(
            first("/usr/local/lib/node_modules/@mulham28/trelloc/lib/bin/trelloc"),
            "npm"
        );
        assert_eq!(
            first("/opt/homebrew/Cellar/trelloc/1.1.0/bin/trelloc"),
            "brew"
        );
        assert_eq!(
            first("C:\\Users\\me\\scoop\\apps\\trelloc\\current\\trelloc.exe"),
            "scoop"
        );
        assert_eq!(first("/home/me/.cargo/bin/trelloc"), "cargo");
        if !cfg!(windows) {
            assert_eq!(first("/home/me/.local/bin/trelloc"), "sh");
        }
    }
}
