//! card bulk move|update|archive|comment, card batch.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::json;
use trelloc_core::{
    ops::{self, BatchOp, BulkChanges},
    Resolver,
};

use super::{card::FilterArgs, file_or_stdin, Ctx};
use crate::output::{confirm, print_json, print_table};

#[derive(Debug, Subcommand)]
pub enum BulkCmd {
    /// Move every matching card to a list
    Move {
        #[command(flatten)]
        filter: FilterArgs,
        /// List name or Board/List
        #[arg(long)]
        to: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Change labels, members, due date or done on every matching card
    Update {
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long = "add-label")]
        add_labels: Vec<String>,
        #[arg(long = "remove-label")]
        remove_labels: Vec<String>,
        #[arg(long = "add-member")]
        add_members: Vec<String>,
        #[arg(long = "remove-member")]
        remove_members: Vec<String>,
        /// Date, or none
        #[arg(long)]
        due: Option<String>,
        #[arg(long = "mark-done", conflicts_with = "mark_not_done")]
        mark_done: bool,
        #[arg(long = "mark-not-done")]
        mark_not_done: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Archive every matching card
    Archive {
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long)]
        dry_run: bool,
    },
    /// Comment on every matching card
    Comment {
        #[command(flatten)]
        filter: FilterArgs,
        /// Comment text (Markdown; - reads stdin). `--text` is the filter.
        #[arg(long)]
        message: String,
        #[arg(long)]
        dry_run: bool,
    },
}

pub async fn bulk(ctx: &Ctx, r: &Resolver<'_>, cmd: BulkCmd) -> Result<()> {
    let (filter, changes, dry_run) = match cmd {
        BulkCmd::Move {
            filter,
            to,
            dry_run,
        } => (
            filter,
            BulkChanges {
                list: Some(to),
                ..Default::default()
            },
            dry_run,
        ),
        BulkCmd::Update {
            filter,
            add_labels,
            remove_labels,
            add_members,
            remove_members,
            due,
            mark_done,
            mark_not_done,
            dry_run,
        } => (
            filter,
            BulkChanges {
                labels_add: add_labels,
                labels_remove: remove_labels,
                members_add: add_members,
                members_remove: remove_members,
                due,
                due_complete: (mark_done || mark_not_done).then_some(mark_done),
                ..Default::default()
            },
            dry_run,
        ),
        BulkCmd::Archive { filter, dry_run } => (
            filter,
            BulkChanges {
                archive: Some(true),
                ..Default::default()
            },
            dry_run,
        ),
        BulkCmd::Comment {
            filter,
            message,
            dry_run,
        } => (
            filter,
            BulkChanges {
                comment: Some(super::text_or_stdin(&message)?),
                ..Default::default()
            },
            dry_run,
        ),
    };
    if changes.is_empty() {
        bail!("no changes given");
    }
    let mut input = filter.input();
    input.limit = Some(trelloc_core::client::PAGE_SIZE);
    let res = ops::search_cards(r, &input).await?;
    let plan = changes.describe().join(", ");
    if dry_run || res.cards.is_empty() {
        if ctx.json {
            return print_json(&json!({
                "dry_run": true,
                "changes": changes.describe(),
                "cards": res.cards.iter().map(|c| ops::card_summary(c, res.snapshot(&c.id_board), None)).collect::<Vec<_>>(),
            }));
        }
        println!("{} card(s) match; would {plan}:", res.cards.len());
        print_table(
            &super::card::CARD_HEADERS,
            res.cards
                .iter()
                .map(|c| super::card::card_row(c, res.snapshot(&c.id_board), None)),
        );
        return Ok(());
    }
    if res.cards.len() > 1
        && !confirm(
            ctx.yes,
            &format!("Apply to {} cards: {plan}?", res.cards.len()),
        )?
    {
        return Ok(());
    }
    let pb = progress(res.cards.len());
    let ids: Vec<String> = res.cards.iter().map(|c| c.id.clone()).collect();
    let results = ops::bulk_update(r, ids, &changes, || pb.inc(1)).await?;
    pb.finish_and_clear();
    report(ctx, &results)
}

fn progress(n: usize) -> indicatif::ProgressBar {
    let pb = if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        indicatif::ProgressBar::new(n as u64)
    } else {
        indicatif::ProgressBar::hidden()
    };
    pb.set_style(
        indicatif::ProgressStyle::with_template("{bar:30} {pos}/{len} {msg}")
            .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar()),
    );
    pb
}

fn report(ctx: &Ctx, results: &[ops::ItemResult]) -> Result<()> {
    let summary = ops::bulk_summary(results);
    if ctx.json {
        return print_json(&summary);
    }
    for r in results.iter().filter(|r| !r.ok) {
        eprintln!(
            "failed {}: {}",
            r.card,
            r.error.as_deref().unwrap_or_default()
        );
    }
    for r in results {
        for w in &r.warnings {
            eprintln!("warning {}: {w}", r.card);
        }
    }
    println!(
        "{} ok, {} failed (of {}).",
        summary["ok"], summary["failed"], summary["total"]
    );
    if summary["failed"] != 0 {
        bail!("some cards failed");
    }
    Ok(())
}

pub async fn batch(ctx: &Ctx, r: &Resolver<'_>, file: &str, dry_run: bool) -> Result<()> {
    let text = file_or_stdin(file)?;
    let ops_list: Vec<BatchOp> =
        serde_json::from_str(&text).context("batch file must be a JSON array of ops")?;
    if dry_run {
        let plan: Vec<String> = ops_list.iter().map(BatchOp::describe).collect();
        if ctx.json {
            return print_json(&json!({ "dry_run": true, "ops": plan }));
        }
        for (i, p) in plan.iter().enumerate() {
            println!("{:>3}. {p}", i + 1);
        }
        return Ok(());
    }
    let destructive = ops_list.iter().filter(|o| o.is_destructive()).count();
    if (ops_list.len() > 1 || destructive > 0)
        && !confirm(
            ctx.yes,
            &format!(
                "Run {} operations ({destructive} permanent deletes)?",
                ops_list.len()
            ),
        )?
    {
        return Ok(());
    }
    let pb = progress(ops_list.len());
    let mut out = Vec::new();
    // Sequential on purpose: later ops may depend on earlier ones.
    for op in &ops_list {
        let res = ops::run_batch_op(r, op).await;
        pb.inc(1);
        out.push(match res {
            Ok(v) => json!({ "op": op.describe(), "ok": true, "result": v }),
            Err(e) => json!({ "op": op.describe(), "ok": false, "error": e.to_string() }),
        });
    }
    pb.finish_and_clear();
    let failed = out.iter().filter(|v| v["ok"] == false).count();
    if ctx.json {
        print_json(&json!({ "total": out.len(), "failed": failed, "results": out }))?;
    } else {
        for v in &out {
            println!(
                "{} {}{}",
                if v["ok"] == true { "ok    " } else { "FAILED" },
                v["op"].as_str().unwrap_or_default(),
                v["error"]
                    .as_str()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            );
        }
    }
    if failed > 0 {
        bail!("{failed} of {} operations failed", out.len());
    }
    Ok(())
}
