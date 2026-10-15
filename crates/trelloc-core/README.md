# trelloc-core

[![crates.io](https://img.shields.io/crates/v/trelloc-core.svg)](https://crates.io/crates/trelloc-core)
[![docs.rs](https://img.shields.io/docsrs/trelloc-core)](https://docs.rs/trelloc-core)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/mulhamna/trello-commands/blob/main/LICENSE)

Async Rust client for the Trello REST API — the library behind the
[`trelloc`](https://crates.io/crates/trelloc) CLI and MCP server.

- **Typed client** — members, workspaces, boards, lists, cards, comments, checklists, labels, custom fields, attachments and activity, plus `/1/batch` for several reads in one request.
- **Credentials stay out of URLs** — the API key and token go only in the `Authorization: OAuth …` header and are redacted from every error message.
- **Rate limits handled** — a shared client-side limiter paces requests (per-token 10 s window, the stricter `/members` and search budgets), reads `x-rate-limit-*` headers, and backs off on 429.
- **Card references** — parse ids, short links, `trello.com/c/…` URLs and `Board#42` / `#42`.
- **Resolver** — names → ids for boards (name, short link, URL, alias), lists (`"Board/List"`), labels (name or color), members (`me`, username, full name) and custom fields with dropdown options; board schemas are cached on disk, with "did you mean" suggestions on typos.
- **Human input** — `parse_when("fri")`, `"+3d"`, `"eow"`, `"none"`; custom-field values from option names, numbers, dates and booleans.
- **High-level operations** — search with filters, create, update, move (warns about labels/fields a cross-board move drops), copy, archive, bulk, batch, standup and board summaries.

```toml
[dependencies]
trelloc-core = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Search with human input

```rust,no_run
use trelloc_core::{
    ops::{self, SearchInput},
    Resolver, TrelloClient, TrelloConfig,
};

#[tokio::main]
async fn main() -> trelloc_core::Result<()> {
    // Same profile file and TRELLO_* env vars as the trelloc CLI.
    let cfg = TrelloConfig::load()?;
    let client = TrelloClient::new(&cfg)?;
    let resolver = Resolver::new(&client, &cfg);

    let found = ops::search_cards(
        &resolver,
        &SearchInput {
            boards: vec!["Roadmap".into()],
            members: vec!["me".into()],
            due_before: Some("eow".into()),
            ..Default::default()
        },
    )
    .await?;

    for card in &found.cards {
        let snap = found.snapshot(&card.id_board);
        println!("{} {}", ops::card_ref(card, snap), card.name);
    }
    Ok(())
}
```

## Move a card by name

```rust,no_run
use trelloc_core::{ops, Resolver, TrelloClient, TrelloConfig};

async fn ship(card: &str) -> trelloc_core::Result<()> {
    let cfg = TrelloConfig::with_credentials("api-key", "token", "https://api.trello.com/1");
    let client = TrelloClient::new(&cfg)?;
    let resolver = Resolver::new(&client, &cfg);

    // "Roadmap#42", a short link or a card URL; "Done" is resolved on the card's board.
    let out = ops::move_card(&resolver, card, "Done", Some("top")).await?;
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }
    println!("{}", out.card.short_url);
    Ok(())
}
```

Errors are a single [`TrelloError`](https://docs.rs/trelloc-core/latest/trelloc_core/error/enum.TrelloError.html) enum (`Auth`, `NotFound`, `Api { status, .. }`, `RateLimit { limit }`, `Resolve { suggestions, .. }`, …); keys and tokens are redacted from every message.

Part of [trello-commands](https://github.com/mulhamna/trello-commands). MIT licensed. Not affiliated with or endorsed by Atlassian or Trello.
