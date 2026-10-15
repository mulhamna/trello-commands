//! comment, checklist, label, field, attach, attachment.

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;
use trelloc_core::ops::{self, ItemUpdate};

use super::{text_or_stdin, Ctx};
use crate::output::{confirm, print_json, print_table};

#[derive(Debug, Subcommand)]
pub enum CommentCmd {
    /// Latest comments on a card (newest first)
    List {
        card: String,
        /// Every comment, not just the latest 50
        #[arg(long)]
        all: bool,
    },
    /// Add a comment (Markdown; `-` reads stdin)
    Add { card: String, text: String },
    /// Replace a comment's text
    Edit {
        card: String,
        comment_id: String,
        text: String,
    },
    /// Delete a comment
    Delete { comment_id: String },
}

#[derive(Debug, Subcommand)]
pub enum ChecklistCmd {
    /// Checklists on a card with progress
    List { card: String },
    /// Add a checklist, optionally with items
    Add {
        card: String,
        name: String,
        #[arg(long = "item")]
        items: Vec<String>,
    },
    /// Delete a checklist (by name or id)
    Delete { card: String, checklist: String },
    /// Checklist items
    #[command(subcommand)]
    Item(ItemCmd),
}

#[derive(Debug, Subcommand)]
pub enum ItemCmd {
    /// Add an item to a checklist
    Add {
        card: String,
        checklist: String,
        text: String,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        member: Option<String>,
    },
    /// Mark an item done (by id or text)
    Check { card: String, item: String },
    /// Mark an item not done
    Uncheck { card: String, item: String },
    /// Rename an item
    Rename {
        card: String,
        item: String,
        text: String,
    },
    /// Delete an item
    Delete { card: String, item: String },
}

#[derive(Debug, Subcommand)]
pub enum LabelCmd {
    /// Labels on a board
    List {
        #[arg(long)]
        board: Option<String>,
    },
    /// Create a label
    Create {
        name: String,
        /// green, yellow, orange, red, purple, blue, sky, lime, pink, black (+ _dark/_light), or none
        #[arg(long)]
        color: String,
        #[arg(long)]
        board: Option<String>,
    },
    /// Rename or recolor a label
    Edit {
        label: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long)]
        board: Option<String>,
    },
    /// Delete a label from the board (and every card)
    Delete {
        label: String,
        #[arg(long)]
        board: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum FieldCmd {
    /// Custom fields on a board, with dropdown options
    List {
        #[arg(long)]
        board: Option<String>,
    },
    /// Set a value: dropdown option name, number, date, true/false or text
    Set {
        card: String,
        field: String,
        value: String,
    },
    /// Clear a value
    Clear { card: String, field: String },
}

#[derive(Debug, Subcommand)]
pub enum AttachmentCmd {
    /// Attachments on a card
    List { card: String },
    /// Delete an attachment
    Delete { card: String, attachment_id: String },
}

pub async fn comment(ctx: &Ctx, cmd: CommentCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    let tz = s.cfg.timezone.as_deref();
    match cmd {
        CommentCmd::List { card, all } => {
            let list = ops::comment_list(&r, &card, all.then_some(usize::MAX)).await?;
            let rows: Vec<_> = list.iter().map(|a| ops::comment_json(a, tz)).collect();
            if ctx.json {
                return print_json(&rows);
            }
            for c in rows {
                println!(
                    "@{} · {} · {}\n{}\n",
                    c["author"].as_str().unwrap_or_default(),
                    c["date"].as_str().unwrap_or_default(),
                    c["id"].as_str().unwrap_or_default(),
                    c["text"].as_str().unwrap_or_default()
                );
            }
        }
        CommentCmd::Add { card, text } => {
            let a = ops::comment_add(&r, &card, &text_or_stdin(&text)?).await?;
            if ctx.json {
                return print_json(&ops::comment_json(&a, tz));
            }
            println!("Commented ({}).", a.id);
        }
        CommentCmd::Edit {
            card,
            comment_id,
            text,
        } => {
            let a = ops::comment_edit(&r, &card, &comment_id, &text_or_stdin(&text)?).await?;
            if ctx.json {
                return print_json(&ops::comment_json(&a, tz));
            }
            println!("Updated comment {comment_id}.");
        }
        CommentCmd::Delete { comment_id } => {
            if !confirm(ctx.yes, &format!("Delete comment {comment_id}?"))? {
                return Ok(());
            }
            s.client.delete_comment(comment_id.trim()).await?;
            println!("Deleted comment {comment_id}.");
        }
    }
    Ok(())
}

pub async fn checklist(ctx: &Ctx, cmd: ChecklistCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    match cmd {
        ChecklistCmd::List { card } => {
            let lists = ops::checklist_list(&r, &card).await?;
            let v: Vec<_> = lists.iter().map(ops::checklist_json).collect();
            if ctx.json {
                return print_json(&v);
            }
            for c in &v {
                println!(
                    "{} ({}/{})",
                    c["name"].as_str().unwrap_or_default(),
                    c["done"],
                    c["total"]
                );
                for i in c["items"].as_array().into_iter().flatten() {
                    println!(
                        "  [{}] {}",
                        if i["done"] == true { "x" } else { " " },
                        i["name"].as_str().unwrap_or_default()
                    );
                }
            }
        }
        ChecklistCmd::Add { card, name, items } => {
            let id = r.card(&card).await?;
            let cl = ops::add_checklist(&r, &id, &name, &items).await?;
            if ctx.json {
                return print_json(&ops::checklist_json(&cl));
            }
            println!(
                "Added checklist '{}' with {} items.",
                cl.name,
                cl.check_items.len()
            );
        }
        ChecklistCmd::Delete { card, checklist } => {
            if !confirm(ctx.yes, &format!("Delete checklist '{checklist}'?"))? {
                return Ok(());
            }
            let cl = ops::checklist_delete(&r, &card, &checklist).await?;
            println!("Deleted checklist '{}'.", cl.name);
        }
        ChecklistCmd::Item(cmd) => {
            let (item, verb) = match cmd {
                ItemCmd::Add {
                    card,
                    checklist,
                    text,
                    due,
                    member,
                } => (
                    ops::checklist_item_add(
                        &r,
                        &card,
                        &checklist,
                        &text,
                        due.as_deref(),
                        member.as_deref(),
                    )
                    .await?,
                    "Added",
                ),
                ItemCmd::Check { card, item } => (
                    ops::checklist_item_update(
                        &r,
                        &card,
                        &item,
                        &ItemUpdate {
                            checked: Some(true),
                            ..Default::default()
                        },
                    )
                    .await?,
                    "Checked",
                ),
                ItemCmd::Uncheck { card, item } => (
                    ops::checklist_item_update(
                        &r,
                        &card,
                        &item,
                        &ItemUpdate {
                            checked: Some(false),
                            ..Default::default()
                        },
                    )
                    .await?,
                    "Unchecked",
                ),
                ItemCmd::Rename { card, item, text } => (
                    ops::checklist_item_update(
                        &r,
                        &card,
                        &item,
                        &ItemUpdate {
                            text: Some(text),
                            ..Default::default()
                        },
                    )
                    .await?,
                    "Renamed",
                ),
                ItemCmd::Delete { card, item } => {
                    if !confirm(ctx.yes, &format!("Delete checklist item '{item}'?"))? {
                        return Ok(());
                    }
                    (
                        ops::checklist_item_delete(&r, &card, &item).await?,
                        "Deleted",
                    )
                }
            };
            if ctx.json {
                return print_json(&item);
            }
            println!("{verb} '{}'.", item.name);
        }
    }
    Ok(())
}

pub async fn label(ctx: &Ctx, cmd: LabelCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    let (label, verb) = match cmd {
        LabelCmd::List { board } => {
            let labels = ops::label_list(&r, board.as_deref()).await?;
            if ctx.json {
                return print_json(&labels);
            }
            print_table(
                &["ID", "Name", "Color"],
                labels.iter().map(|l| {
                    vec![
                        l.id.clone(),
                        l.name.clone(),
                        l.color.clone().unwrap_or_default(),
                    ]
                }),
            );
            return Ok(());
        }
        LabelCmd::Create { name, color, board } => (
            ops::label_create(&r, board.as_deref(), &name, &color).await?,
            "Created",
        ),
        LabelCmd::Edit {
            label,
            name,
            color,
            board,
        } => (
            ops::label_update(
                &r,
                board.as_deref(),
                &label,
                name.as_deref(),
                color.as_deref(),
            )
            .await?,
            "Updated",
        ),
        LabelCmd::Delete { label, board } => {
            if !confirm(
                ctx.yes,
                &format!("Delete label '{label}' from the board and every card?"),
            )? {
                return Ok(());
            }
            (
                ops::label_delete(&r, board.as_deref(), &label).await?,
                "Deleted",
            )
        }
    };
    if ctx.json {
        return print_json(&label);
    }
    println!(
        "{verb} label {} ({}).",
        label.display(),
        label.color.as_deref().unwrap_or("no color")
    );
    Ok(())
}

pub async fn field(ctx: &Ctx, cmd: FieldCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    match cmd {
        FieldCmd::List { board } => {
            let fields = ops::field_list(&r, board.as_deref()).await?;
            if ctx.json {
                return print_json(&fields);
            }
            print_table(
                &["ID", "Name", "Type", "Options"],
                fields.iter().map(|f| {
                    vec![
                        f.id.clone(),
                        f.name.clone(),
                        f.field_type.clone(),
                        f.options
                            .iter()
                            .map(|o| o.value.text.clone())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ]
                }),
            );
        }
        FieldCmd::Set { card, field, value } => {
            let note = ops::set_card_field(&r, &card, &field, &value).await?;
            if ctx.json {
                return print_json(&json!({ "result": note }));
            }
            println!("{note}");
        }
        FieldCmd::Clear { card, field } => {
            if !confirm(ctx.yes, &format!("Clear '{field}' on {card}?"))? {
                return Ok(());
            }
            let note = ops::clear_card_field(&r, &card, &field).await?;
            println!("{note}");
        }
    }
    Ok(())
}

pub async fn attach(
    ctx: &Ctx,
    card: String,
    targets: Vec<String>,
    name: Option<String>,
    cover: bool,
) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    let mut out = Vec::new();
    for t in &targets {
        let a = ops::attachment_add(&r, &card, t, name.as_deref(), cover).await?;
        if !ctx.json {
            println!("Attached {} ({})", a.name, a.id);
        }
        out.push(a);
    }
    if ctx.json {
        print_json(&out)?;
    }
    Ok(())
}

pub async fn attachment(ctx: &Ctx, cmd: AttachmentCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    match cmd {
        AttachmentCmd::List { card } => {
            let list = ops::attachment_list(&r, &card).await?;
            if ctx.json {
                return print_json(&list);
            }
            print_table(
                &["ID", "Name", "Size", "URL"],
                list.iter().map(|a| {
                    vec![
                        a.id.clone(),
                        a.name.clone(),
                        a.bytes.map(|b| format!("{b} B")).unwrap_or_default(),
                        a.url.clone(),
                    ]
                }),
            );
        }
        AttachmentCmd::Delete {
            card,
            attachment_id,
        } => {
            if !confirm(ctx.yes, &format!("Delete attachment {attachment_id}?"))? {
                return Ok(());
            }
            ops::attachment_delete(&r, &card, &attachment_id).await?;
            println!("Deleted attachment {attachment_id}.");
        }
    }
    Ok(())
}
