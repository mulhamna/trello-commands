# @mulham28/trelloc

[![npm](https://img.shields.io/npm/v/@mulham28/trelloc.svg)](https://www.npmjs.com/package/@mulham28/trelloc)
[![CI](https://github.com/mulhamna/trello-commands/actions/workflows/ci.yml/badge.svg)](https://github.com/mulhamna/trello-commands/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/mulhamna/trello-commands/blob/main/LICENSE)

**Trello from your terminal — and from your AI agent.** A fast native CLI plus an MCP server with 48 tools, in one binary.

```bash
npm i -g @mulham28/trelloc
trelloc auth login                      # API key + token (opens Trello's authorize page)
trelloc mcp install --client claude-code
```

Before login, create a Trello app without Power-Up capabilities and generate its API key. Follow the [authentication setup guide](https://github.com/mulhamna/trello-commands/blob/main/INSTALL.md#configure-trello-access).

Interactive MCP setup asks whether to expose all boards or only selected boards. Manage active boards per profile with `trelloc board access`; see the [board-access guide](https://github.com/mulhamna/trello-commands/blob/main/INSTALL.md#configure-trello-access).

## What you get

```bash
trelloc card list --mine --overdue
trelloc card create "Fix login redirect" --list Doing --label bug --due fri
trelloc card update roadmap#42 --add-member ana --field 'Severity=Low'
trelloc card move roadmap#42 --to "Ops/Backlog"          # warns about labels lost across boards
trelloc card bulk move --list Review --to Done --dry-run
trelloc standup
```

- **Names, not ids** — boards, lists, labels, people (`me`, username) and custom fields resolved for you, with typo suggestions. Cards by id, shortLink, URL or `board#42`.
- **Human dates** — `fri`, `tomorrow`, `+3d`, `eow`, `2026-10-20 14:00`.
- **MCP server** — `trelloc mcp serve` exposes 48 `trello_*` tools to Claude, Cursor, Codex, Gemini CLI, VS Code and others. Its board access follows the active-board policy for the selected profile; deletes require explicit confirmation.
- **Rate-limit aware** — paces itself under Trello's 100 requests / 10 s token limit and caches board schemas, so bulk work doesn't trip 429s.

## Use it from an MCP client

```json
{
  "mcpServers": {
    "trelloc": { "command": "trelloc", "args": ["mcp", "serve"] }
  }
}
```

Or skip `-g` and let the client fetch it:

```json
{
  "mcpServers": {
    "trelloc": { "command": "npx", "args": ["-y", "@mulham28/trelloc", "mcp", "serve"] }
  }
}
```

The API key and token stay in the trelloc config (or `TRELLO_API_KEY` / `TRELLO_TOKEN`) — they are never written into client configs.

## How the package works

On install, a small script downloads the prebuilt `trelloc` binary for your platform (Linux, macOS, Windows · x86_64 / arm64) from the matching [GitHub Release](https://github.com/mulhamna/trello-commands/releases) and verifies its SHA-256 against the release's `checksums.txt`. Releases are built in GitHub Actions and published to npm with provenance via trusted publishing. If the binary is missing (e.g. installed with `--ignore-scripts`), run `npm rebuild -g @mulham28/trelloc`.

## More

[Full README & CLI tour](https://github.com/mulhamna/trello-commands#readme) · [MCP tools](https://github.com/mulhamna/trello-commands/blob/main/docs/MCP.md) · [Changelog](https://github.com/mulhamna/trello-commands/blob/main/CHANGELOG.md) · [Issues](https://github.com/mulhamna/trello-commands/issues)

Other installs: Homebrew `brew install mulhamna/tap/trelloc` · Scoop `scoop install mulhamna/trelloc` · Cargo `cargo install trelloc`.

MIT licensed. Not affiliated with or endorsed by Atlassian or Trello.
