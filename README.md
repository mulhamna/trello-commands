# trelloc

**Unofficial** Trello CLI and MCP server in one Rust binary. Sibling of [jira-commands](https://github.com/mulhamna/jira-commands).

> Status: pre-alpha, under construction. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

One download gives you both:

- **`trelloc …`** — a fast CLI for boards, lists, cards, comments, checklists, labels, custom fields and bulk operations.
- **`trelloc mcp serve`** — an MCP server so Claude, Cursor, Codex, Gemini CLI and friends can work in your Trello.

## Why not Trello's official MCP server?

| | Official (`mcp.trello.com/v1`) | trelloc |
| --- | --- | --- |
| Auth | OAuth only | Your own API key + token |
| Workspaces per connection | One | Every workspace your token can see |
| Permanent delete | No (archive only) | Yes, with explicit confirmation (archive stays the default) |
| Comments, attachments, custom fields, label editing, card copy, members | Planned | Yes |
| Local file upload | No | Yes |
| CLI for humans and scripts | No | Yes, same binary |
| Raw API escape hatch | No | `trelloc api` / `trello_api_request` |

Comparison based on the official server's README as of October 2026.

Trello's REST limits (100 requests / 10 s per token, 300 / 10 s per key, and a tighter limit on member routes) are paced client-side, so bulk work doesn't trip 429s.

## Install

> Coming with the first release. Every channel installs the same single binary.

```bash
npm i -g @mulham28/trelloc        # npm
cargo install trelloc             # crates.io
# brew / scoop / install.sh — planned
```

## Quick start

```bash
trelloc auth login                     # walks you through creating an API key and token
trelloc board list
trelloc board use Roadmap

trelloc card list --list Doing --mine
trelloc card create "Fix login redirect" --list "To Do" --label bug --due fri
trelloc card move Roadmap#42 --to Done
trelloc comment add Roadmap#42 "Deployed to staging"
trelloc checklist add Roadmap#42 "Release" --item "Changelog" --item "Tag"
```

Cards can be referenced by id, short link, URL, or `board#number`. Boards, lists, labels, members and custom fields can be referenced by name, with "did you mean" suggestions on typos.

## Use with an AI client

```bash
trelloc mcp install --client claude-code   # or claude-desktop, cursor, codex, gemini-cli, vscode, …
trelloc mcp doctor
```

Or by hand:

```json
{
  "mcpServers": {
    "trello": { "command": "trelloc", "args": ["mcp", "serve"] }
  }
}
```

Credentials stay in `~/.config/trelloc/config.toml` (or `TRELLO_API_KEY` / `TRELLO_TOKEN`); they're never written into client configs.

## Configuration

| Env var | Purpose |
| --- | --- |
| `TRELLO_API_KEY` | API key (from your Power-Up admin page) |
| `TRELLO_TOKEN` | User token |
| `TRELLO_PROFILE` | Profile name in `config.toml` |
| `TRELLO_DEFAULT_BOARD` | Board used when `--board` is omitted |
| `TRELLO_WORKSPACE` | Default workspace for board lookups |
| `TRELLO_TIMEOUT_SECS` | HTTP timeout |

## Docs

- [Roadmap](docs/ROADMAP.md) · [Architecture](docs/ARCHITECTURE.md) · [CLI spec](docs/CLI.md) · [MCP tools](docs/MCP.md)
- [Trello API notes](docs/TRELLO-API.md) · [Onboarding](docs/ONBOARDING.md)

## License

MIT. Trello is a trademark of Atlassian; this project is not affiliated with or endorsed by Atlassian.
