//! standup, summary, activity, api.

use anyhow::{Context, Result};
use serde_json::Value;
use trelloc_core::ops::{self, ActivityInput};

use super::{file_or_stdin, Ctx};
use crate::output::{confirm, print_json, print_table};

pub async fn standup(
    ctx: &Ctx,
    since: String,
    board: Option<String>,
    member: Option<String>,
) -> Result<()> {
    let s = ctx.session()?;
    let r = s.resolver();
    let st = ops::standup(&r, &since, board.as_deref(), member.as_deref()).await?;
    if ctx.json {
        return print_json(&st);
    }
    println!("Standup for @{} since {}\n", st.member, st.since);
    let name = |v: &Value| v["name"].as_str().unwrap_or_default().to_string();
    let section = |title: &str, items: Vec<String>| {
        if !items.is_empty() {
            println!("{title}:");
            for i in items {
                println!("  - {i}");
            }
            println!();
        }
    };
    section(
        "Moved",
        st.moved
            .iter()
            .map(|m| {
                format!(
                    "{} ({} → {})",
                    name(&m["card"]),
                    m["from"].as_str().unwrap_or("?"),
                    m["to"].as_str().unwrap_or("?")
                )
            })
            .collect(),
    );
    section("Completed", st.completed.iter().map(name).collect());
    section("Commented", st.commented.iter().map(name).collect());
    let with_ref = |v: &Value| {
        format!(
            "{}  {}  [{}]",
            v["ref"].as_str().unwrap_or_default(),
            name(v),
            v["list"]["name"].as_str().unwrap_or_default()
        )
    };
    section("Overdue", st.overdue.iter().map(with_ref).collect());
    section("Due soon", st.due_soon.iter().map(with_ref).collect());
    section("In progress", st.in_progress.iter().map(with_ref).collect());
    Ok(())
}

pub async fn summary(ctx: &Ctx, board: Option<String>) -> Result<()> {
    let s = ctx.session()?;
    let v = ops::board_report(&s.resolver(), board.as_deref()).await?;
    if ctx.json {
        return print_json(&v);
    }
    println!(
        "{} — {} open cards ({} overdue, {} without due date, {} done)\n",
        v["board"]["name"].as_str().unwrap_or_default(),
        v["total"],
        v["overdue"],
        v["no_due"],
        v["done"]
    );
    print_table(
        &["List", "Cards"],
        v["per_list"].as_array().into_iter().flatten().map(|l| {
            vec![
                l["list"].as_str().unwrap_or_default().to_string(),
                l["cards"].to_string(),
            ]
        }),
    );
    print_table(
        &["Member", "Cards"],
        v["per_member"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, n)| vec![k.clone(), n.to_string()]),
    );
    Ok(())
}

pub async fn activity(ctx: &Ctx, input: ActivityInput) -> Result<()> {
    let s = ctx.session()?;
    let list = ops::activity(&s.resolver(), &input).await?;
    if ctx.json {
        return print_json(&list);
    }
    print_table(
        &["Date", "By", "Type", "Card", "Detail"],
        list.iter().map(|a| {
            let detail = match (a["list_before"].as_str(), a["list_after"].as_str()) {
                (Some(b), Some(af)) => format!("{b} → {af}"),
                _ => a["text"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(60)
                    .collect(),
            };
            vec![
                a["date"].as_str().unwrap_or_default().to_string(),
                format!("@{}", a["by"].as_str().unwrap_or_default()),
                a["type"].as_str().unwrap_or_default().to_string(),
                a["card"].as_str().unwrap_or_default().to_string(),
                detail,
            ]
        }),
    );
    Ok(())
}

pub async fn api(
    ctx: &Ctx,
    method: String,
    path: String,
    query: Option<String>,
    data: Option<String>,
) -> Result<()> {
    let method = method.to_ascii_uppercase();
    let body = match data {
        Some(d) => {
            let text = match d.strip_prefix('@') {
                Some(file) => file_or_stdin(file)?,
                None => d,
            };
            Some(serde_json::from_str::<Value>(&text).context("--data must be JSON")?)
        }
        None => None,
    };
    if method != "GET" && !confirm(ctx.yes, &format!("Send {method} {path}?"))? {
        return Ok(());
    }
    let s = ctx.session()?;
    let v = s.client.raw(&method, &path, query.as_deref(), body).await?;
    print_json(&v)
}
