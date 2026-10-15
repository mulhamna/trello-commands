//! card list|view|create|update|move|copy|done|undone|archive|unarchive|delete|open.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use trelloc_core::{
    model::{BoardSnapshot, Card},
    ops::{self, CardInput, ChecklistInput, SearchInput, UpdateInput, WriteResult},
    time::format_iso,
    Resolver,
};

use super::{file_or_stdin, Ctx};
use crate::output::{confirm, is_tty, print_json, print_table};

#[derive(Debug, Clone, Default, Args)]
pub struct FilterArgs {
    /// Board(s) to scan (id, shortLink, URL, alias or name)
    #[arg(long = "board")]
    pub boards: Vec<String>,
    /// List(s): name (on --board or the default board), id, or Board/List
    #[arg(long = "list")]
    pub lists: Vec<String>,
    /// Assigned member(s): me, username, full name or id (all must match)
    #[arg(long = "member")]
    pub members: Vec<String>,
    /// Cards you're on
    #[arg(long)]
    pub mine: bool,
    /// Label name or color (all must match)
    #[arg(long = "label")]
    pub labels: Vec<String>,
    #[arg(long)]
    pub due_before: Option<String>,
    #[arg(long)]
    pub due_after: Option<String>,
    /// Due in the past and not marked done
    #[arg(long)]
    pub overdue: bool,
    #[arg(long)]
    pub no_due: bool,
    /// Due date marked complete
    #[arg(long, conflicts_with = "not_done")]
    pub done: bool,
    #[arg(long)]
    pub not_done: bool,
    #[arg(long)]
    pub created_after: Option<String>,
    #[arg(long)]
    pub created_before: Option<String>,
    #[arg(long)]
    pub active_after: Option<String>,
    #[arg(long)]
    pub active_before: Option<String>,
    /// attachments | description | members | cover
    #[arg(long = "has")]
    pub has: Vec<String>,
    /// Custom field filter: 'Name=value', 'Points>=3', 'Notes~text', 'Name empty'
    #[arg(long = "field")]
    pub fields: Vec<String>,
    /// Name or description contains
    #[arg(long)]
    pub text: Option<String>,
    /// Raw Trello search, e.g. '@me label:bug due:week' (forces search mode)
    #[arg(long)]
    pub query: Option<String>,
    /// Include archived cards
    #[arg(long)]
    pub archived: bool,
}

impl FilterArgs {
    pub fn input(&self) -> SearchInput {
        SearchInput {
            boards: self.boards.clone(),
            lists: self.lists.clone(),
            members: self.members.clone(),
            mine: self.mine,
            labels: self.labels.clone(),
            due_before: self.due_before.clone(),
            due_after: self.due_after.clone(),
            overdue: self.overdue,
            no_due: self.no_due,
            due_complete: if self.done {
                Some(true)
            } else if self.not_done {
                Some(false)
            } else {
                None
            },
            created_after: self.created_after.clone(),
            created_before: self.created_before.clone(),
            active_after: self.active_after.clone(),
            active_before: self.active_before.clone(),
            has: self.has.clone(),
            fields: self.fields.clone(),
            text: self.text.clone(),
            query: self.query.clone(),
            include_archived: self.archived,
            ..Default::default()
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum CardCmd {
    /// List or search cards
    List {
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long)]
        limit: Option<usize>,
        /// due | created | activity | pos
        #[arg(long)]
        sort: Option<String>,
        #[arg(long)]
        reverse: bool,
    },
    /// Show a card: lists, members, labels, dates, fields, checklists, attachments
    View {
        card: Option<String>,
        /// Add the latest comments
        #[arg(long)]
        comments: bool,
        /// Add the latest activity
        #[arg(long)]
        activity: bool,
        /// Open in the browser instead
        #[arg(long)]
        web: bool,
    },
    /// Create a card
    Create {
        name: String,
        /// List name (on --board / the default board), id, or Board/List
        #[arg(long)]
        list: Option<String>,
        #[arg(long)]
        board: Option<String>,
        /// Markdown description
        #[arg(long, conflicts_with = "desc_file")]
        desc: Option<String>,
        /// Description from a file, or - for stdin
        #[arg(long)]
        desc_file: Option<String>,
        /// top | bottom
        #[arg(long)]
        pos: Option<String>,
        #[arg(long = "member")]
        members: Vec<String>,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        start: Option<String>,
        /// Custom field: 'Name=value' (repeatable)
        #[arg(long = "field")]
        fields: Vec<String>,
        /// Checklist: 'Name:item1;item2' (repeatable)
        #[arg(long = "checklist")]
        checklists: Vec<String>,
        /// Copy another card
        #[arg(long)]
        from: Option<String>,
        /// With --from: all, or e.g. checklists,labels,attachments
        #[arg(long, requires = "from")]
        keep: Option<String>,
        /// Open the new card in the browser
        #[arg(long)]
        open: bool,
    },
    /// Change a card
    Update {
        card: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "desc_file")]
        desc: Option<String>,
        #[arg(long)]
        desc_file: Option<String>,
        /// Date, or none
        #[arg(long)]
        due: Option<String>,
        /// Date, or none
        #[arg(long)]
        start: Option<String>,
        #[arg(long, conflicts_with = "not_done")]
        done: bool,
        #[arg(long)]
        not_done: bool,
        #[arg(long = "add-member")]
        add_members: Vec<String>,
        #[arg(long = "remove-member")]
        remove_members: Vec<String>,
        #[arg(long = "add-label")]
        add_labels: Vec<String>,
        #[arg(long = "remove-label")]
        remove_labels: Vec<String>,
        /// Custom field: 'Name=value' (repeatable)
        #[arg(long = "field")]
        fields: Vec<String>,
        #[arg(long = "clear-field")]
        clear_fields: Vec<String>,
        #[arg(long, conflicts_with = "unwatch")]
        watch: bool,
        #[arg(long)]
        unwatch: bool,
        /// Cover color, or none
        #[arg(long)]
        cover: Option<String>,
    },
    /// Move to another list or board (warns about labels/fields that won't survive)
    Move {
        card: Option<String>,
        /// List name, or Board/List
        #[arg(long)]
        to: String,
        /// top | bottom
        #[arg(long)]
        pos: Option<String>,
    },
    /// Copy a card into a list
    Copy {
        card: Option<String>,
        #[arg(long)]
        to: String,
        #[arg(long)]
        name: Option<String>,
        /// all (default), or e.g. checklists,labels,attachments
        #[arg(long)]
        keep: Option<String>,
    },
    /// Mark the due date complete
    Done { card: Option<String> },
    /// Mark the due date not complete
    Undone { card: Option<String> },
    /// Archive cards (reversible)
    Archive {
        #[arg(required = true)]
        cards: Vec<String>,
    },
    /// Restore archived cards
    Unarchive {
        #[arg(required = true)]
        cards: Vec<String>,
    },
    /// Delete cards permanently
    Delete {
        #[arg(required = true)]
        cards: Vec<String>,
    },
    /// Open a card in the browser
    Open { card: Option<String> },
    /// Bulk edits over filtered cards (dry-run first with --dry-run)
    #[command(subcommand)]
    Bulk(super::bulk::BulkCmd),
    /// Mixed operations from a JSON file
    Batch {
        /// JSON array of {"op": "create"|"update"|"move"|"comment"|"archive"|"delete", …}; - for stdin
        #[arg(long)]
        file: String,
        #[arg(long)]
        dry_run: bool,
    },
}

/// `Name=value` → map, keeping the first `=`.
pub fn field_pairs(items: &[String]) -> Result<BTreeMap<String, String>> {
    items
        .iter()
        .map(|f| {
            let (k, v) = f
                .split_once('=')
                .with_context(|| format!("--field '{f}' must look like 'Name=value'"))?;
            Ok((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Missing card on a TTY → pick from the default board.
async fn card_or_pick(r: &Resolver<'_>, card: Option<String>) -> Result<String> {
    if let Some(c) = card {
        return Ok(c);
    }
    crate::output::require_tty("card", "<card>")?;
    let res = ops::search_cards(r, &SearchInput::default()).await?;
    if res.cards.is_empty() {
        bail!("no cards on the default board; pass <card>");
    }
    let labels: Vec<String> = res
        .cards
        .iter()
        .map(|c| {
            let snap = res.snapshot(&c.id_board);
            format!(
                "{}  {}  [{}]",
                ops::card_ref(c, snap),
                c.name,
                snap.and_then(|s| s.list_name(&c.id_list)).unwrap_or("")
            )
        })
        .collect();
    let pick = inquire::Select::new("Card:", labels.clone()).prompt()?;
    let i = labels.iter().position(|l| *l == pick).unwrap_or(0);
    Ok(res.cards[i].id.clone())
}

pub fn card_row(c: &Card, snap: Option<&BoardSnapshot>, tz: Option<&str>) -> Vec<String> {
    vec![
        ops::card_ref(c, snap),
        c.name.clone(),
        snap.and_then(|s| s.list_name(&c.id_list))
            .unwrap_or(&c.id_list)
            .to_string(),
        c.id_members
            .iter()
            .map(|m| {
                snap.and_then(|s| s.member(m))
                    .map(|x| format!("@{}", x.username))
                    .unwrap_or_else(|| m.clone())
            })
            .collect::<Vec<_>>()
            .join(" "),
        c.labels
            .iter()
            .map(|l| l.display())
            .collect::<Vec<_>>()
            .join(", "),
        c.due
            .as_deref()
            .map(|d| format_iso(d, tz))
            .unwrap_or_default(),
        if c.due_complete { "✓" } else { "" }.to_string(),
    ]
}

pub const CARD_HEADERS: [&str; 7] = ["Ref", "Name", "List", "Members", "Labels", "Due", "✓"];

fn report(ctx: &Ctx, out: &WriteResult, snap: Option<&BoardSnapshot>, verb: &str) -> Result<()> {
    let tz = ctx.config().ok().and_then(|c| c.timezone);
    let tz = tz.as_deref();
    if ctx.json {
        let mut v = ops::card_summary(&out.card, snap, tz);
        v["notes"] = json!(out.notes);
        v["warnings"] = json!(out.warnings);
        return print_json(&v);
    }
    println!(
        "{verb} {}  {}  {}",
        ops::card_ref(&out.card, snap),
        out.card.name,
        out.card.short_url
    );
    for n in &out.notes {
        println!("  · {n}");
    }
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

async fn snap_of(r: &Resolver<'_>, card: &Card) -> Option<BoardSnapshot> {
    r.snapshot(&trelloc_core::resolve::BoardId {
        id: card.id_board.clone(),
        name: card.id_board.clone(),
    })
    .await
    .ok()
}

fn desc_arg(desc: Option<String>, file: Option<String>) -> Result<Option<String>> {
    match (desc, file) {
        (Some(d), _) => Ok(Some(super::text_or_stdin(&d)?)),
        (None, Some(f)) => Ok(Some(file_or_stdin(&f)?)),
        (None, None) => Ok(None),
    }
}

pub async fn run(ctx: &Ctx, cmd: CardCmd) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    let tz = s.cfg.timezone.clone();
    let tz = tz.as_deref();
    match cmd {
        CardCmd::List {
            filter,
            limit,
            sort,
            reverse,
        } => {
            let mut input = filter.input();
            input.limit = Some(limit.unwrap_or(s.cfg.default_card_limit));
            input.sort = sort;
            input.reverse = reverse;
            let res = ops::search_cards(&r, &input).await?;
            if ctx.json {
                return print_json(
                    &res.cards
                        .iter()
                        .map(|c| ops::card_summary(c, res.snapshot(&c.id_board), tz))
                        .collect::<Vec<_>>(),
                );
            }
            if res.mode == "search" {
                eprintln!("searched via Trello search; results may lag a few seconds");
            }
            print_table(
                &CARD_HEADERS,
                res.cards
                    .iter()
                    .map(|c| card_row(c, res.snapshot(&c.id_board), tz)),
            );
        }
        CardCmd::View {
            card,
            comments,
            activity,
            web,
        } => {
            let card = card_or_pick(&r, card).await?;
            let (c, snap) = ops::get_card(&r, &card, true).await?;
            if web {
                open::that(&c.url).context("opening the browser")?;
                return Ok(());
            }
            let mut v = ops::card_detail(&c, snap.as_ref(), tz);
            if comments {
                let list = ops::comment_list(&r, &c.id, Some(10)).await?;
                v["comments"] = json!(list
                    .iter()
                    .map(|a| ops::comment_json(a, tz))
                    .collect::<Vec<_>>());
            }
            if activity {
                let acts = s.client.card_actions(&c.id, None, 0).await?;
                v["activity"] = json!(acts
                    .iter()
                    .take(20)
                    .map(|a| json!({ "type": a.action_type, "date": a.date, "by": a.author() }))
                    .collect::<Vec<_>>());
            }
            if ctx.json {
                return print_json(&v);
            }
            print_card(&v);
        }
        CardCmd::Create {
            name,
            list,
            board,
            desc,
            desc_file,
            pos,
            members,
            labels,
            due,
            start,
            fields,
            checklists,
            from,
            keep,
            open,
        } => {
            let list = match (list, &s.cfg.default_list) {
                (Some(l), _) => Some(l),
                (None, Some(_)) => None,
                (None, None) if is_tty() => {
                    let b = r.board_or_default(board.as_deref()).await?;
                    let snap = r.snapshot(&b).await?;
                    let names: Vec<String> = snap.lists.iter().map(|l| l.name.clone()).collect();
                    Some(inquire::Select::new("List:", names).prompt()?)
                }
                (None, None) => bail!("missing list; pass --list or set default_list"),
            };
            let input = CardInput {
                name,
                list,
                board,
                desc: desc_arg(desc, desc_file)?,
                pos,
                members,
                labels,
                due,
                start,
                fields: field_pairs(&fields)?,
                checklists: checklists
                    .iter()
                    .map(|c| ChecklistInput::parse(c))
                    .collect(),
                copy_from: from,
                keep,
            };
            let out = ops::create_card(&r, &input).await?;
            let snap = snap_of(&r, &out.card).await;
            report(ctx, &out, snap.as_ref(), "Created")?;
            if open {
                let _ = open::that(&out.card.url);
            }
        }
        CardCmd::Update {
            card,
            name,
            desc,
            desc_file,
            due,
            start,
            done,
            not_done,
            add_members,
            remove_members,
            add_labels,
            remove_labels,
            fields,
            clear_fields,
            watch,
            unwatch,
            cover,
        } => {
            let card = card_or_pick(&r, card).await?;
            let input = UpdateInput {
                name,
                desc: desc_arg(desc, desc_file)?,
                due,
                start,
                due_complete: (done || not_done).then_some(done),
                members_add: add_members,
                members_remove: remove_members,
                labels_add: add_labels,
                labels_remove: remove_labels,
                fields: field_pairs(&fields)?,
                fields_clear: clear_fields,
                subscribed: (watch || unwatch).then_some(watch),
                cover,
                closed: None,
            };
            let out = ops::update_card(&r, &card, &input).await?;
            let snap = snap_of(&r, &out.card).await;
            report(ctx, &out, snap.as_ref(), "Updated")?;
        }
        CardCmd::Move { card, to, pos } => {
            let card = card_or_pick(&r, card).await?;
            let out = ops::move_card(&r, &card, &to, pos.as_deref()).await?;
            let snap = snap_of(&r, &out.card).await;
            report(ctx, &out, snap.as_ref(), "Moved")?;
        }
        CardCmd::Copy {
            card,
            to,
            name,
            keep,
        } => {
            let card = card_or_pick(&r, card).await?;
            let out = ops::copy_card(&r, &card, &to, name.as_deref(), keep.as_deref()).await?;
            let snap = snap_of(&r, &out.card).await;
            report(ctx, &out, snap.as_ref(), "Copied")?;
        }
        CardCmd::Done { card } => set_done(ctx, &r, card, true).await?,
        CardCmd::Undone { card } => set_done(ctx, &r, card, false).await?,
        CardCmd::Archive { cards } => archive(ctx, &r, cards, false).await?,
        CardCmd::Unarchive { cards } => archive(ctx, &r, cards, true).await?,
        CardCmd::Delete { cards } => {
            let prompt = format!(
                "Permanently delete {} card(s)? This can't be undone (archive is reversible)",
                cards.len()
            );
            if !confirm(ctx.yes, &prompt)? {
                return Ok(());
            }
            let mut out = Vec::new();
            for c in &cards {
                let id = ops::delete_card(&r, c).await?;
                if !ctx.json {
                    println!("Deleted {c} ({id})");
                }
                out.push(json!({ "card": c, "id": id, "deleted": true }));
            }
            if ctx.json {
                print_json(&out)?;
            }
        }
        CardCmd::Open { card } => {
            let card = card_or_pick(&r, card).await?;
            let (c, _) = ops::get_card(&r, &card, false).await?;
            open::that(&c.url).context("opening the browser")?;
            println!("{}", c.url);
        }
        CardCmd::Bulk(cmd) => super::bulk::bulk(ctx, &r, cmd).await?,
        CardCmd::Batch { file, dry_run } => super::bulk::batch(ctx, &r, &file, dry_run).await?,
    }
    Ok(())
}

async fn set_done(ctx: &Ctx, r: &Resolver<'_>, card: Option<String>, done: bool) -> Result<()> {
    let card = card_or_pick(r, card).await?;
    let out = ops::update_card(
        r,
        &card,
        &UpdateInput {
            due_complete: Some(done),
            ..Default::default()
        },
    )
    .await?;
    let snap = snap_of(r, &out.card).await;
    report(
        ctx,
        &out,
        snap.as_ref(),
        if done { "Done:" } else { "Not done:" },
    )
}

async fn archive(ctx: &Ctx, r: &Resolver<'_>, cards: Vec<String>, unarchive: bool) -> Result<()> {
    let mut out = Vec::new();
    for c in &cards {
        let card = ops::archive_card(r, c, unarchive).await?;
        if !ctx.json {
            println!(
                "{} {}  {}",
                if unarchive { "Restored" } else { "Archived" },
                c,
                card.name
            );
        }
        out.push(json!({ "id": card.id, "name": card.name, "closed": card.closed }));
    }
    if ctx.json {
        print_json(&out)?;
    }
    Ok(())
}

fn print_card(v: &Value) {
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    println!("{}  ({})", s("name"), s("ref"));
    println!("{}", s("url"));
    println!(
        "Board: {}   List: {}",
        v["board"]["name"].as_str().unwrap_or_default(),
        v["list"]["name"].as_str().unwrap_or_default()
    );
    let names = |key: &str, field: &str, prefix: &str| {
        v[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| {
                x[field]
                    .as_str()
                    .filter(|n| !n.is_empty())
                    .map(|n| format!("{prefix}{n}"))
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let members = names("members", "username", "@");
    if !members.is_empty() {
        println!("Members: {members}");
    }
    let labels = v["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            l["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .or(l["color"].as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(", ");
    if !labels.is_empty() {
        println!("Labels:  {labels}");
    }
    for (k, label) in [("start", "Start"), ("due", "Due")] {
        if let Some(d) = v[k].as_str() {
            println!(
                "{label}:{}{d}{}",
                " ".repeat(8 - label.len()),
                if k == "due" && v["due_complete"] == true {
                    "  ✓ done"
                } else {
                    ""
                }
            );
        }
    }
    for f in v["custom_fields"].as_array().into_iter().flatten() {
        println!(
            "{}: {}",
            f["name"].as_str().unwrap_or_default(),
            f["value"].as_str().unwrap_or_default()
        );
    }
    let desc = s("desc");
    if !desc.trim().is_empty() {
        println!("\n{desc}");
    }
    for c in v["checklists"].as_array().into_iter().flatten() {
        println!(
            "\n☐ {} ({}/{})",
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
    let att = v["attachments"].as_array().map_or(0, Vec::len);
    if att > 0 {
        println!("\nAttachments:");
        for a in v["attachments"].as_array().into_iter().flatten() {
            println!(
                "  {}  {}",
                a["name"].as_str().unwrap_or_default(),
                a["url"].as_str().unwrap_or_default()
            );
        }
    }
    for c in v["comments"].as_array().into_iter().flatten() {
        println!(
            "\n@{} · {}\n{}",
            c["author"].as_str().unwrap_or_default(),
            c["date"].as_str().unwrap_or_default(),
            c["text"].as_str().unwrap_or_default()
        );
    }
    for a in v["activity"].as_array().into_iter().flatten() {
        println!(
            "  {}  {}  @{}",
            a["date"].as_str().unwrap_or_default(),
            a["type"].as_str().unwrap_or_default(),
            a["by"].as_str().unwrap_or_default()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_pairs_split_on_first_equals() {
        let m = field_pairs(&["Severity=High".into(), "Formula = a=b".into()]).unwrap();
        assert_eq!(m["Severity"], "High");
        assert_eq!(m["Formula"], "a=b");
        assert!(field_pairs(&["nope".into()]).is_err());
    }
}
