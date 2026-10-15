# trelloc-mcp

[![crates.io](https://img.shields.io/crates/v/trelloc-mcp.svg)](https://crates.io/crates/trelloc-mcp)
[![docs.rs](https://img.shields.io/docsrs/trelloc-mcp)](https://docs.rs/trelloc-mcp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/mulhamna/trello-commands/blob/main/LICENSE)

The Trello [Model Context Protocol](https://modelcontextprotocol.io) server behind `trelloc mcp serve`, as an embeddable library built on [`rmcp`](https://crates.io/crates/rmcp).

> **Just want to use it?** Install the [`trelloc`](https://crates.io/crates/trelloc) binary and run `trelloc mcp install` — this crate is for embedding the server in your own Rust program.

- **47 `trello_*` tools** — boards and lists, cards, bulk & batch, comments, checklists, labels, custom fields, attachments, reports, and a raw API escape hatch.
- **Every workspace, one connection** — tools see every board the token can see; optional `profile` / `workspace` per call switch account or narrow lookups.
- **Model-friendly inputs** — names or ids for boards, lists, labels, members and custom-field options; cards by short link, URL or `Board#42`; dates like `tomorrow`, `+3d`, `eow`.
- **Compact outputs** — card summaries by default (`detail: "full"` on request), so results don't flood the model's context.
- **Safe** — delete / remove / clear tools return `unsafe_operation` unless `confirm: true`; archive stays the default.
- **Stable errors** — `data.code` is one of `validation_error`, `unsafe_operation`, `auth_missing`, `config_error`, `resolve_error` (+ `suggestions`), `trello_api_error` (+ `status`), `rate_limited` (+ `limit`), `unavailable`.
- **Two transports** — stdio and Streamable HTTP.

```toml
[dependencies]
trelloc-mcp = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
anyhow = "1"
```

```rust,no_run
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Reads the trelloc profile / TRELLO_* env vars on every tool call.
    trelloc_mcp::run_stdio().await
    // or: trelloc_mcp::run_streamable_http("127.0.0.1", 8787, "/mcp").await
}
```

`TrelloMcpServer` implements `rmcp::ServerHandler` if you need to serve it over your own transport.

Full tool reference: [docs/MCP.md](https://github.com/mulhamna/trello-commands/blob/main/docs/MCP.md). Part of [trello-commands](https://github.com/mulhamna/trello-commands). MIT licensed. Not affiliated with or endorsed by Atlassian or Trello.
