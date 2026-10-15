# trelloc

[![crates.io](https://img.shields.io/crates/v/trelloc.svg)](https://crates.io/crates/trelloc)
[![npm](https://img.shields.io/npm/v/@mulham28/trelloc.svg?label=npm)](https://www.npmjs.com/package/@mulham28/trelloc)
[![CI](https://github.com/mulhamna/trello-commands/actions/workflows/ci.yml/badge.svg)](https://github.com/mulhamna/trello-commands/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/mulhamna/trello-commands/blob/main/LICENSE)

**Trello from your terminal — and from your AI agent.** One binary: a fast CLI for humans and scripts, plus an MCP server with 47 tools (`trelloc mcp serve`).

```bash
cargo install trelloc
trelloc auth login            # API key + token: the command walks you through both
trelloc card list --board Roadmap --mine --overdue
trelloc mcp install           # register the MCP server with Claude, Cursor, Codex, …
```

## Highlights

- **Names, not ids** — boards, lists, labels, members and custom-field options are resolved for you, with "did you mean" suggestions.
- **Every way to point at a card** — id, short link, `trello.com/c/…` URL, or `Board#42`.
- **Human dates** — `--due fri`, `--due-before eow`, `--active-after -2w`, `--due none`.
- **Cards end to end** — search with filters (list, member, label, dates, custom fields, text), create, update, move across boards with warnings, copy, archive, checklists, comments, attachments.
- **Bulk & batch** — `card bulk move|update|archive|comment` with `--dry-run`, mixed operations from a JSON file, bounded concurrency.
- **Boards & reports** — create and edit boards and lists, `standup`, `summary`, `activity`.
- **Rate-limit aware** — client-side pacing for Trello's 10 s windows and the stricter `/members` and search limits; cached board schemas.
- **Safe & scriptable** — archive by default, deletes confirm unless `--yes`, `--json` on every read, stable exit codes, `trelloc api` for raw `/1` calls.

## MCP server

```bash
trelloc mcp install --client claude-code   # or cursor, codex, gemini-cli, vscode, claude-desktop, … (17 targets)
```

```json
{ "mcpServers": { "trelloc": { "command": "trelloc", "args": ["mcp", "serve"] } } }
```

47 `trello_*` tools across boards, lists, cards, bulk, comments, checklists, labels, custom fields, attachments and reports. Inputs accept names and human dates; destructive tools require `confirm: true`; the key and token are read from the trelloc config and never written into client configs. Stdio by default, Streamable HTTP with `--transport streamable-http`.

## Other ways to install

Homebrew `brew install mulhamna/tap/trelloc` · Scoop `scoop install mulhamna/trelloc` · npm `npm i -g @mulham28/trelloc` · [install script and release archives](https://github.com/mulhamna/trello-commands#install).

## Links

[README & CLI tour](https://github.com/mulhamna/trello-commands#readme) · [CLI spec](https://github.com/mulhamna/trello-commands/blob/main/docs/CLI.md) · [MCP tools](https://github.com/mulhamna/trello-commands/blob/main/docs/MCP.md) · [Changelog](https://github.com/mulhamna/trello-commands/blob/main/CHANGELOG.md)

MIT licensed. Not affiliated with or endorsed by Atlassian or Trello.
