# trelloc-mcp

[![crates.io](https://img.shields.io/crates/v/trelloc-mcp.svg)](https://crates.io/crates/trelloc-mcp)
[![docs.rs](https://img.shields.io/docsrs/trelloc-mcp)](https://docs.rs/trelloc-mcp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/mulhamna/trello-commands/blob/main/LICENSE)

The Trello [Model Context Protocol](https://modelcontextprotocol.io) server behind `trelloc mcp serve`, as an embeddable library built on [`rmcp`](https://crates.io/crates/rmcp).

> **Just want to use it?** Install the [`trelloc`](https://crates.io/crates/trelloc) binary and run `trelloc mcp install` — this crate is for embedding the server in your own Rust program.

```rust,no_run
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Reads the trelloc profile / TRELLO_* env vars on every tool call.
    trelloc_mcp::run_stdio().await
    // or: trelloc_mcp::run_streamable_http("127.0.0.1", 8787, "/mcp").await
}
```

`TrelloMcpServer` implements `rmcp::ServerHandler` if you need to serve it over your own transport.

Part of [trello-commands](https://github.com/mulhamna/trello-commands). MIT licensed. Not affiliated with or endorsed by Atlassian or Trello.
