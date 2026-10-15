//! api (reports arrive later).

use anyhow::{Context, Result};
use serde_json::Value;

use super::{file_or_stdin, Ctx};
use crate::output::{confirm, print_json};

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
