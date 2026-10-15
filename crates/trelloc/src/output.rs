//! Table / JSON rendering and interactive helpers. JSON output is the
//! stable scripting interface; table layout may change.

use std::io::IsTerminal;

use anyhow::{bail, Result};
use comfy_table::{presets::UTF8_HORIZONTAL_ONLY, ContentArrangement, Table};
use serde::Serialize;

pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn table<I, R, S>(headers: &[&str], rows: I) -> Table
where
    I: IntoIterator<Item = R>,
    R: IntoIterator<Item = S>,
    S: Into<comfy_table::Cell>,
{
    let mut t = Table::new();
    t.load_preset(UTF8_HORIZONTAL_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(headers.iter().copied());
    for row in rows {
        t.add_row(row);
    }
    t
}

pub fn print_table<I, R, S>(headers: &[&str], rows: I)
where
    I: IntoIterator<Item = R>,
    R: IntoIterator<Item = S>,
    S: Into<comfy_table::Cell>,
{
    println!("{}", table(headers, rows));
}

/// Prompts render on stderr (inquire), so only stdin and stderr must be a
/// terminal; stdout may be piped.
pub fn is_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Error naming the flag when we can't prompt.
pub fn require_tty(what: &str, flag: &str) -> Result<()> {
    if !is_tty() {
        bail!("missing {what}; pass {flag}");
    }
    Ok(())
}

/// Ask before a destructive action; `--yes` skips, non-TTY refuses.
pub fn confirm(yes: bool, prompt: &str) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !is_tty() {
        bail!("{prompt} — refusing without confirmation; pass --yes");
    }
    Ok(inquire::Confirm::new(prompt).with_default(false).prompt()?)
}

/// Spinner on stderr while waiting on the network; hidden when not a TTY.
pub fn spinner(msg: &str) -> indicatif::ProgressBar {
    let pb = if std::io::stderr().is_terminal() {
        indicatif::ProgressBar::new_spinner()
    } else {
        indicatif::ProgressBar::hidden()
    };
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}
