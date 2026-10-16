//! Manage which Trello boards are available to a profile.

use anyhow::Result;
use clap::Subcommand;
use std::collections::BTreeMap;
use trelloc_core::{
    config::{BoardAccess, BoardAccessFile, ConfigFile, TrelloConfig},
    resolve::Resolver,
    TrelloClient,
};

use super::Ctx;
use crate::output::{print_json, print_table, require_tty};

#[derive(Debug, Subcommand)]

pub enum BoardAccessCmd {
    /// Show board activation state
    List,
    /// Allow every board
    All,
    /// Activate a board (by id or name)
    Activate { board: String },
    /// Deactivate a board (by id or name)
    Deactivate { board: String },
    /// Interactively choose the active boards
    Select,
}

fn profile_name(ctx: &Ctx, file: &ConfigFile) -> String {
    ctx.profile
        .clone()
        .unwrap_or_else(|| file.selected_profile_name())
}

pub async fn run(ctx: &Ctx, cmd: BoardAccessCmd) -> Result<()> {
    let file = ConfigFile::load()?;
    let name = profile_name(ctx, &file);
    let mut access_file = BoardAccessFile::load()?;
    let mut access = access_file.access(&name);

    match cmd {
        BoardAccessCmd::List => {
            let cfg = TrelloConfig::from_file(&file, Some(&name))?;
            let client = TrelloClient::new(&cfg)?;
            let resolver = Resolver::new(&client, &cfg).with_board_access(BoardAccess::All);
            let directory = resolver.directory(ctx.refresh).await?;
            let rows: Vec<_> = directory
                .boards
                .iter()
                .map(|b| (b, access.is_active(&b.id)))
                .collect();
            if ctx.json {
                let values: Vec<_> = rows
                    .iter()
                    .map(|(b, active)| {
                        let mut value = trelloc_core::ops::board_summary(b);
                        value["active"] = serde_json::Value::Bool(*active);
                        value
                    })
                    .collect();
                return print_json(&values);
            }
            print_table(
                &["Board", "ID", "Active"],
                rows.iter().map(|(b, active)| {
                    vec![
                        b.name.clone(),
                        b.id.clone(),
                        if *active { "yes".into() } else { "no".into() },
                    ]
                }),
            );
        }
        BoardAccessCmd::All => {
            access.set_all();
            access_file.set(name.clone(), access);
            access_file.save()?;
            println!("All boards are active for profile '{name}'.");
        }
        BoardAccessCmd::Activate { board } => {
            let id = resolve_board(ctx, &name, &board).await?;
            access.activate(id.clone());
            access_file.set(name.clone(), access);
            access_file.save()?;
            println!("Activated board for profile '{name}'.");
        }
        BoardAccessCmd::Deactivate { board } => {
            let id = resolve_board(ctx, &name, &board).await?;
            access.deactivate(id.clone());
            access_file.set(name.clone(), access);
            access_file.save()?;
            println!("Deactivated board for profile '{name}'.");
        }
        BoardAccessCmd::Select => {
            require_tty("board selection", "board access activate <board> or all")?;
            let cfg = TrelloConfig::from_file(&file, Some(&name))?;
            let client = TrelloClient::new(&cfg)?;
            let resolver = Resolver::new(&client, &cfg).with_board_access(BoardAccess::All);
            let directory = resolver.directory(ctx.refresh).await?;
            let board_ids: BTreeMap<String, String> = directory
                .boards
                .iter()
                .map(|board| (format!("{} ({})", board.name, board.id), board.id.clone()))
                .collect();
            let labels: Vec<String> = board_ids.keys().cloned().collect();
            let defaults: Vec<usize> = labels
                .iter()
                .enumerate()
                .filter_map(|(i, label)| {
                    board_ids
                        .get(label)
                        .filter(|id| access.is_active(id))
                        .map(|_| i)
                })
                .collect();
            let selected = inquire::MultiSelect::new("Select active boards", labels.clone())
                .with_default(&defaults)
                .prompt()
                .map_err(|e| anyhow::anyhow!("board selection cancelled: {e}"))?;
            let ids = selected
                .iter()
                .filter_map(|label| board_ids.get(label).cloned())
                .collect::<Vec<_>>();
            access.select(ids);
            access_file.set(name.clone(), access);
            access_file.save()?;
            println!("Updated active boards for profile '{name}'.");
        }
    }
    Ok(())
}

async fn resolve_board(ctx: &Ctx, profile: &str, input: &str) -> Result<String> {
    let cfg = TrelloConfig::load_profile(Some(profile))?;
    let client = TrelloClient::new(&cfg)?;
    let resolver = Resolver::new(&client, &cfg)
        .with_refresh(ctx.refresh)
        .with_workspace(ctx.workspace.clone())
        .with_board_access(BoardAccess::All);
    Ok(resolver.board(input).await?.id)
}
