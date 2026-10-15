//! board, list, tree, alias, member, cache.

use anyhow::{bail, Result};
use clap::Subcommand;
use trelloc_core::{
    client::Filter,
    config::ConfigFile,
    ops::{self, BoardListInput, BoardUpdate, ListUpdate},
    resolve::suggest,
};

use super::Ctx;
use crate::output::{confirm, print_json, print_table};

#[derive(Debug, Subcommand)]
pub enum BoardCmd {
    /// Boards you can see
    List {
        #[arg(long)]
        closed: bool,
        #[arg(long)]
        starred: bool,
    },
    /// Lists (with card counts), labels, members and custom fields
    View { board: Option<String> },
    /// Set the profile's default board
    Use { board: String },
    /// Create a board
    Create {
        name: String,
        /// Copy lists, labels and cards from this board
        #[arg(long)]
        from: Option<String>,
        /// Don't add To Do / Doing / Done
        #[arg(long)]
        no_default_lists: bool,
    },
    /// Rename a board
    Rename { board: String, name: String },
    /// Archive (close) a board
    Archive { board: String },
    /// Restore an archived board
    Unarchive { board: String },
    /// Delete a board permanently, with all lists and cards
    Delete { board: String },
}

#[derive(Debug, Subcommand)]
pub enum ListCmd {
    /// Lists on a board
    List {
        #[arg(long)]
        board: Option<String>,
        /// Include archived lists
        #[arg(long)]
        closed: bool,
    },
    /// Create a list
    Create {
        name: String,
        #[arg(long)]
        board: Option<String>,
        /// top | bottom | number
        #[arg(long)]
        pos: Option<String>,
    },
    /// Rename a list
    Rename {
        list: String,
        name: String,
        #[arg(long)]
        board: Option<String>,
    },
    /// Archive a list
    Archive {
        list: String,
        #[arg(long)]
        board: Option<String>,
    },
    /// Restore an archived list (by id)
    Unarchive {
        list: String,
        #[arg(long)]
        board: Option<String>,
    },
    /// Move every card in a list to another list
    MoveCards {
        list: String,
        /// List name or Board/List
        #[arg(long)]
        to: String,
        #[arg(long)]
        board: Option<String>,
    },
    /// Archive every card in a list
    ArchiveCards {
        list: String,
        #[arg(long)]
        board: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AliasCmd {
    /// Map a short name to a board or `board/list` (stored in the profile)
    Set { name: String, target: String },
    /// List aliases
    List,
    /// Delete an alias
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
pub enum CacheCmd {
    /// Delete cached boards and board snapshots
    Clear,
}

#[derive(Debug, Subcommand)]
pub enum MemberCmd {
    /// Members of a board (default board when omitted), or of the
    /// workspace given with the global --workspace
    List {
        #[arg(long)]
        board: Option<String>,
    },
}

pub async fn board(ctx: &Ctx, cmd: BoardCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    match cmd {
        BoardCmd::List { closed, starred } => {
            let boards = ops::board_list(
                &r,
                &BoardListInput {
                    workspace: ctx.workspace.clone(),
                    include_closed: closed,
                    starred,
                },
            )
            .await?;
            if ctx.json {
                return print_json(&boards.iter().map(ops::board_summary).collect::<Vec<_>>());
            }
            let dir = r.directory(false).await?;
            print_table(
                &["Short link", "Name", "Workspace", ""],
                boards.iter().map(|b| {
                    vec![
                        b.short_link.clone(),
                        b.name.clone(),
                        dir.workspace_name(b.workspace_id.as_deref()),
                        [b.starred.then_some("★"), b.closed.then_some("archived")]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ]
                }),
            );
        }
        BoardCmd::View { board } => {
            let b = r.board_or_default(board.as_deref()).await?;
            let v = ops::board_overview(&r, &b.id).await?;
            if ctx.json {
                return print_json(&v);
            }
            println!(
                "{}  ({})",
                v["name"].as_str().unwrap_or_default(),
                v["url"].as_str().unwrap_or_default()
            );
            println!();
            print_table(
                &["List", "Cards"],
                v["lists"].as_array().into_iter().flatten().map(|l| {
                    vec![
                        l["name"].as_str().unwrap_or_default().to_string(),
                        l["cards"].to_string(),
                    ]
                }),
            );
            let join = |key: &str, f: &dyn Fn(&serde_json::Value) -> String| {
                v[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(f)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            println!(
                "Labels:  {}",
                join(
                    "labels",
                    &|l| match (l["name"].as_str(), l["color"].as_str()) {
                        (Some(n), Some(c)) if !n.is_empty() => format!("{n} ({c})"),
                        (Some(n), None) if !n.is_empty() => n.to_string(),
                        (_, Some(c)) => c.to_string(),
                        _ => String::new(),
                    }
                )
            );
            println!(
                "Members: {}",
                join("members", &|m| format!(
                    "@{}",
                    m["username"].as_str().unwrap_or_default()
                ))
            );
            let fields = join("custom_fields", &|f| {
                format!(
                    "{} ({})",
                    f["name"].as_str().unwrap_or_default(),
                    f["type"].as_str().unwrap_or_default()
                )
            });
            if !fields.is_empty() {
                println!("Fields:  {fields}");
            }
        }
        BoardCmd::Use { board } => {
            let b = r.board(&board).await?;
            let mut file = ConfigFile::load()?;
            let name = ctx
                .profile
                .clone()
                .unwrap_or_else(|| file.selected_profile_name());
            file.profile_mut(&name).default_board = Some(b.id.clone());
            file.save()?;
            println!("Default board for profile '{name}': {} ({}).", b.name, b.id);
        }
        BoardCmd::Create {
            name,
            from,
            no_default_lists,
        } => {
            let b = ops::board_create(
                &r,
                &name,
                ctx.workspace.as_deref(),
                from.as_deref(),
                !no_default_lists,
            )
            .await?;
            if ctx.json {
                return print_json(&ops::board_summary(&b));
            }
            println!("Created board {} ({})  {}", b.name, b.short_link, b.url);
        }
        BoardCmd::Rename { board, name } => {
            let b = ops::board_update(
                &r,
                &board,
                &BoardUpdate {
                    name: Some(name),
                    ..Default::default()
                },
            )
            .await?;
            println!("Renamed board to '{}'.", b.name);
        }
        BoardCmd::Archive { board } => {
            if !confirm(ctx.yes, &format!("Archive board '{board}'?"))? {
                return Ok(());
            }
            let b = ops::board_update(
                &r,
                &board,
                &BoardUpdate {
                    closed: Some(true),
                    ..Default::default()
                },
            )
            .await?;
            println!("Archived board '{}'.", b.name);
        }
        BoardCmd::Unarchive { board } => {
            let b = ops::board_update(
                &r,
                &board,
                &BoardUpdate {
                    closed: Some(false),
                    ..Default::default()
                },
            )
            .await?;
            println!("Restored board '{}'.", b.name);
        }
        BoardCmd::Delete { board } => {
            if !confirm(
                ctx.yes,
                &format!("Permanently delete board '{board}' with every list and card?"),
            )? {
                return Ok(());
            }
            let b = ops::board_delete(&r, &board).await?;
            println!("Deleted board '{}'.", b.name);
        }
    }
    Ok(())
}

pub async fn tree(ctx: &Ctx, workspace: Option<String>) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    let tree = ops::tree(&r, workspace.as_deref().or(ctx.workspace.as_deref())).await?;
    if ctx.json {
        return print_json(&tree);
    }
    for ws in &tree {
        println!("{}", ws.name);
        for (i, b) in ws.boards.iter().enumerate() {
            let last = i + 1 == ws.boards.len();
            println!("{} {}  ({})", if last { "└─" } else { "├─" }, b.name, b.id);
            for (j, l) in b.lists.iter().enumerate() {
                println!(
                    "{}  {} {l}",
                    if last { " " } else { "│" },
                    if j + 1 == b.lists.len() {
                        "└─"
                    } else {
                        "├─"
                    }
                );
            }
        }
    }
    Ok(())
}

pub async fn list(ctx: &Ctx, cmd: ListCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    match cmd {
        ListCmd::List { board, closed } => {
            let b = r.board_or_default(board.as_deref()).await?;
            let lists = s
                .client
                .board_lists(&b.id, if closed { Filter::All } else { Filter::Open })
                .await?;
            if ctx.json {
                return print_json(&lists);
            }
            print_table(
                &["ID", "Name", ""],
                lists.iter().map(|l| {
                    vec![
                        l.id.clone(),
                        l.name.clone(),
                        if l.closed { "archived" } else { "" }.to_string(),
                    ]
                }),
            );
        }
        ListCmd::Create { name, board, pos } => {
            let l = ops::list_create(&r, board.as_deref(), &name, pos.as_deref()).await?;
            if ctx.json {
                return print_json(&l);
            }
            println!("Created list '{}' ({}).", l.name, l.id);
        }
        ListCmd::Rename { list, name, board } => {
            let l = ops::list_update(
                &r,
                board.as_deref(),
                &list,
                &ListUpdate {
                    name: Some(name),
                    ..Default::default()
                },
            )
            .await?;
            println!("Renamed list to '{}'.", l.name);
        }
        ListCmd::Archive { list, board } => {
            let l = ops::list_update(
                &r,
                board.as_deref(),
                &list,
                &ListUpdate {
                    closed: Some(true),
                    ..Default::default()
                },
            )
            .await?;
            println!("Archived list '{}' ({}).", l.name, l.id);
        }
        ListCmd::Unarchive { list, board } => {
            let l = ops::list_update(
                &r,
                board.as_deref(),
                &list,
                &ListUpdate {
                    closed: Some(false),
                    ..Default::default()
                },
            )
            .await?;
            println!("Restored list '{}'.", l.name);
        }
        ListCmd::MoveCards { list, to, board } => {
            if !confirm(ctx.yes, &format!("Move every card in '{list}' to '{to}'?"))? {
                return Ok(());
            }
            let (from, tb, tl) = ops::list_move_cards(&r, board.as_deref(), &list, &to).await?;
            println!(
                "Moved all cards from '{}' to {}/{}.",
                from.name, tb.name, tl.name
            );
        }
        ListCmd::ArchiveCards { list, board } => {
            if !confirm(ctx.yes, &format!("Archive every card in '{list}'?"))? {
                return Ok(());
            }
            let l = ops::list_archive_cards(&r, board.as_deref(), &list).await?;
            println!("Archived all cards in '{}'.", l.name);
        }
    }
    Ok(())
}

pub async fn alias(ctx: &Ctx, cmd: AliasCmd) -> Result<()> {
    let mut file = ConfigFile::load()?;
    let name = ctx
        .profile
        .clone()
        .unwrap_or_else(|| file.selected_profile_name());
    match cmd {
        AliasCmd::Set {
            name: alias,
            target,
        } => {
            // Validate now so a typo fails here, not on first use.
            let s = ctx.session()?;
            let r = s.resolver();
            let stored = match target.rsplit_once('/') {
                Some(_) => {
                    let (b, l) = r.list_path(&target, None).await?;
                    format!("{}/{}", b.id, l.name)
                }
                None => r.board(&target).await?.id,
            };
            file.profile_mut(&name)
                .aliases
                .insert(alias.clone(), stored.clone());
            file.save()?;
            println!("Alias '{alias}' → {target} ({stored}).");
        }
        AliasCmd::List => {
            let p = file.profiles.get(&name).cloned().unwrap_or_default();
            if ctx.json {
                return print_json(&p.aliases);
            }
            print_table(
                &["Alias", "Target"],
                p.aliases.iter().map(|(k, v)| vec![k.clone(), v.clone()]),
            );
        }
        AliasCmd::Remove { name: alias } => {
            let p = file.profile_mut(&name);
            if p.aliases.remove(&alias).is_none() {
                let names: Vec<String> = p.aliases.keys().cloned().collect();
                let hint = suggest(&alias, &names);
                if hint.is_empty() {
                    bail!("no alias '{alias}'");
                }
                bail!("no alias '{alias}' (did you mean: {}?)", hint.join(", "));
            }
            file.save()?;
            println!("Removed alias '{alias}'.");
        }
    }
    Ok(())
}

pub async fn cache(ctx: &Ctx, cmd: CacheCmd) -> Result<()> {
    match cmd {
        CacheCmd::Clear => {
            let cfg = ctx.config()?;
            let client = trelloc_core::TrelloClient::new(&cfg)?;
            let r = trelloc_core::Resolver::new(&client, &cfg);
            r.cache().clear()?;
            println!("Cleared {}", r.cache().dir().display());
        }
    }
    Ok(())
}

pub async fn member(ctx: &Ctx, cmd: MemberCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    match cmd {
        MemberCmd::List { board } => {
            let workspace = if board.is_some() {
                None
            } else {
                ctx.workspace.as_deref()
            };
            let members = ops::member_list(&r, board.as_deref(), workspace).await?;
            if ctx.json {
                return print_json(&members);
            }
            print_table(
                &["ID", "Username", "Name"],
                members.iter().map(|m| {
                    vec![
                        m.id.clone(),
                        format!("@{}", m.username),
                        m.full_name.clone(),
                    ]
                }),
            );
        }
    }
    Ok(())
}
