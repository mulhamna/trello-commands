<div align="center">

```
  _            _ _
 | |_ _ __ ___| | | ___   ___
 | __| '__/ _ \ | |/ _ \ / __|
 | |_| | |  __/ | | (_) | (__
  \__|_|  \___|_|_|\___/ \___|
```

**Trello from your terminal — and from your AI agent.**
One Rust binary: a fast CLI for humans and scripts, plus an MCP server with 48 tools.

[![CI](https://github.com/mulhamna/trello-commands/actions/workflows/ci.yml/badge.svg)](https://github.com/mulhamna/trello-commands/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/trelloc.svg?label=crates.io)](https://crates.io/crates/trelloc)
[![npm](https://img.shields.io/npm/v/@mulham28/trelloc.svg?label=npm)](https://www.npmjs.com/package/@mulham28/trelloc)
[![docs.rs](https://img.shields.io/docsrs/trelloc-core?label=docs.rs)](https://docs.rs/trelloc-core)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[Install](#install) · [Quick start](#quick-start) · [CLI tour](#cli-tour) · [MCP server](#mcp-server) · [Configuration](#configuration) · [Docs](#docs)

</div>

---

```console
$ trelloc card list --board Roadmap --mine --overdue
 Ref           Name                        List    Members  Labels  Due         ✓
 Xk3Pq9Lm#42   Fix login redirect loop     Doing   ana      bug     2026-10-14
 Xk3Pq9Lm#57   Rate-limit webhook retries  Review  ana              2026-10-15

$ trelloc card move Roadmap#42 --to "Revieww"
error: Could not resolve list 'Revieww' (did you mean: Review?)
```

## Why trelloc

- **Names, not ids.** Boards, lists, labels, members and custom fields (with dropdown options) are resolved for you — with "did you mean" suggestions and a hard error on ambiguous names, never a silent guess.
- **Every way to point at a card.** Id, short link, `trello.com/c/…` URL, or `Board#42`.
- **Human dates.** `--due fri`, `--due "2026-10-20 14:00"`, `--due-before eow`, `--active-after -2w`, `--due none`.
- **Built for Trello's rate limits.** Requests are paced client-side (100 per 10 s per token, the stricter `/members` and search budgets), board schemas are cached, and bulk work runs with bounded concurrency.
- **Agent-ready.** `trelloc mcp serve` exposes 48 typed `trello_*` tools to Claude, Cursor, Codex, Gemini CLI, VS Code and more; `trelloc mcp install` registers it in one command.
- **Multiple accounts and board scopes.** Each profile has its own credentials and active-board policy; choose all boards or a subset, then activate or deactivate boards later.
- **Safe by default.** Archive is the default; deletes confirm on the CLI (`--yes` to skip) and require `confirm: true` over MCP. Your key and token are never printed, logged, put in a URL, or written into client configs.
- **Scriptable.** `--json` on every read command, stable exit codes, and raw `trelloc api` access while board access is unrestricted.

### trelloc vs Trello's official MCP server

| | Official MCP (`mcp.trello.com/v1`) | trelloc |
| --- | --- | --- |
| Auth | OAuth only | Your own API key + token |
| Board scope | Every board the token can see | All boards or a per-profile active-board selection |
| Permanent delete | No (archive only) | Yes, with explicit confirmation (archive stays the default) |
| Comments, attachments, custom fields, label editing, card copy | Planned | Yes |
| Local file upload | No | Yes |
| A CLI for humans and scripts | No | Yes, same binary |
| Raw API escape hatch | No | `trelloc api` / `trello_api_request` when board access is unrestricted |

Comparison based on the official server's README as of October 2026.

## Install

Every channel installs the same single binary (Linux, macOS, Windows — x86_64 and aarch64).

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.sh | bash
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.ps1 | iex
```

| Channel | Command |
| --- | --- |
| Homebrew | `brew install mulhamna/tap/trelloc` |
| Scoop | `scoop bucket add mulhamna https://github.com/mulhamna/scoop-bucket` then `scoop install mulhamna/trelloc` |
| npm | `npm i -g @mulham28/trelloc` |
| Cargo | `cargo install trelloc` |
| Manual | archives on the [Releases](https://github.com/mulhamna/trello-commands/releases) page |

The install scripts and the npm package verify every download against the release's `checksums.txt` (more options, upgrades and uninstalling: [INSTALL.md](INSTALL.md)); release archives also carry [build provenance attestations](https://github.com/mulhamna/trello-commands/attestations). Pin a version with `TRELLOC_VERSION=v1.0.0` (sh) or `-Version v1.0.0` (PowerShell).

## Quick start

```bash
trelloc auth login                  # enter your app API key, approve and paste the user token
trelloc tree                        # Workspaces → Boards → Lists, with ids
trelloc board use Roadmap           # default board for commands without --board
trelloc alias set doing "Roadmap/Doing"

trelloc card list --list doing --mine
trelloc card create "Fix login redirect" --list "To Do" --label bug --due fri --member me
trelloc card move Roadmap#42 --to Review
trelloc comment add Roadmap#42 "Deployed to staging"
trelloc checklist add Roadmap#42 "Release" --item "Changelog" --item "Tag"
trelloc standup
```

Authentication requires your own Trello app API key and user token. Create an app without Power-Up capabilities; follow the [setup guide](INSTALL.md#configure-trello-access).

Wire it into your AI client:

```bash
trelloc mcp install                 # interactive: checks prerequisites, then pick clients
trelloc mcp install --client claude-code
```

## CLI tour

Cards can be referenced by id (`5f9b…`), short link (`Ab12Cd34`), URL (`https://trello.com/c/Ab12Cd34/…`) or board number (`Roadmap#42`, or `#42` on the default board).

**Find work**

```bash
trelloc card list --mine --overdue
trelloc card list --board Roadmap --label bug --due-before eow --json
trelloc card list --board Roadmap --field 'Points>=3' --sort due --limit 20
trelloc card list --text "login" --archived
trelloc card view Roadmap#42 --comments --activity
```

**Change work**

```bash
trelloc card update Roadmap#42 --add-member ana --due +2d --add-label urgent --field 'Severity=Low'
trelloc card move Roadmap#42 --to "Archive Board/Done"     # warns about labels/fields that won't survive
trelloc card copy Roadmap#42 --to "Q4/Backlog" --keep checklists,labels
trelloc card done Roadmap#42
trelloc attach Roadmap#42 ./screenshot.png https://example.com/spec
trelloc card archive Roadmap#42         # reversible
trelloc card delete Roadmap#42          # permanent; asks first, --yes to skip
```

**Many cards at once**

```bash
trelloc card bulk move --board Roadmap --list Review --to Done --dry-run
trelloc card bulk update --board Roadmap --label bug --add-label triaged --mark-done
trelloc card bulk comment --board Roadmap --overdue --message "Please update the ETA"
trelloc card batch --file ops.json      # mixed create / update / move / comment / archive / delete
```

**Boards, fields, reports**

```bash
trelloc board view Roadmap              # lists with card counts, labels, members, custom fields
trelloc list create "Blocked" --board Roadmap
trelloc list move-cards Review --to Done --board Roadmap
trelloc field list --board Roadmap
trelloc field set Roadmap#42 "Severity" High
trelloc summary --board Roadmap         # counts per list / member, overdue, no due, done
trelloc activity --board Roadmap --since -1d
```

**Escape hatch**

```bash
trelloc api GET /members/me/boards --query 'fields=name,url'
trelloc api PUT /cards/Ab12Cd34 --data @body.json
```

<details>
<summary>All commands</summary>

| Command | What it does |
| --- | --- |
| `auth login \| status \| logout \| profile` | Key, token and profile management |
| `whoami` | Logged-in member (cached; `--refresh` asks Trello) |
| `workspace list \| use` | Workspaces and the default one for board lookups |
| `board …` | `list`, `view`, `use`, `create`, `rename`, `archive`, `unarchive`, `delete` |
| `tree` | Workspace → Board → List tree |
| `list …` | `list`, `create`, `rename`, `archive`, `unarchive`, `move-cards`, `archive-cards` |
| `alias set \| list \| remove` | Short names for boards and lists |
| `member list` | Board or workspace members |
| `card …` | `list`, `view`, `create`, `update`, `move`, `copy`, `done`, `undone`, `archive`, `unarchive`, `delete`, `open`, `bulk`, `batch` |
| `comment …` | `list`, `add`, `edit`, `delete` |
| `checklist …` | `list`, `add`, `delete`, `item add \| check \| uncheck \| rename \| delete` |
| `label …` | `list`, `create`, `edit`, `delete` |
| `field list \| set \| clear` | Custom fields with human values (Custom Fields Power-Up) |
| `attach`, `attachment list \| delete` | Upload files or link URLs; manage attachments |
| `standup`, `summary`, `activity` | Reports |
| `api` | Raw `/1` request |
| `config get \| set` | Profile settings |
| `cache clear` | Drop cached boards and board snapshots |
| `mcp serve \| install \| doctor` | MCP server |

Full spec: [docs/CLI.md](docs/CLI.md).

</details>

## MCP server

`trelloc mcp serve` speaks MCP over **stdio** (default) or **Streamable HTTP** (`--transport streamable-http --port 8787`). It reads the same config and credentials as the CLI.

```bash
trelloc mcp install                         # pick clients and profiles with checkboxes (logs you in first if needed)
trelloc mcp install --client claude-code --client cursor --server-profile work --server-profile personal
trelloc mcp doctor                          # binary on PATH, credentials, client config
```

Or add it by hand:

```json
{
  "mcpServers": {
    "trelloc": { "command": "trelloc", "args": ["mcp", "serve"] }
  }
}
```

| Area | Tools |
| --- | --- |
| Account | `profile_list`, `auth_status`, `whoami`, `workspace_list`, `member_list`, `board_access` |
| Boards & lists | `board_list`, `board_get`, `board_create`, `board_update`, `board_delete`, `list_create`, `list_update` |
| Cards | `card_search`, `card_get`, `card_create`, `card_update`, `card_move`, `card_copy`, `card_archive`, `card_delete` |
| Bulk | `card_bulk_create`, `card_bulk_update`, `batch` |
| Comments | `comment_list`, `comment_add`, `comment_edit`, `comment_delete` |
| Checklists | `checklist_list`, `checklist_add`, `checklist_delete`, `checklist_item_add`, `checklist_item_update`, `checklist_item_delete` |
| Labels | `label_list`, `label_create`, `label_update`, `label_delete` |
| Custom fields | `field_list`, `field_set`, `field_clear` |
| Attachments | `attachment_list`, `attachment_add`, `attachment_delete` |
| Reports | `standup`, `board_summary`, `activity` |
| Escape hatch | `api_request` |

The entry is named `trelloc`, so it sits next to Trello's official server without clashing; pick another name with `--name`. All tool names are prefixed `trello_`. Inputs take names or ids and human dates, plus optional `profile` / `workspace` to switch account or narrow board lookups per call. Destructive tools fail with `unsafe_operation` unless `confirm: true`, and errors carry stable codes (`resolve_error` with suggestions, `trello_api_error` with the HTTP status, `rate_limited` with the limit that was hit). Details: [docs/MCP.md](docs/MCP.md).

## Configuration

`trelloc auth login` writes credentials to `config.toml` (permissions `600`). Board policy lives separately in `board-access.toml` and applies per profile.

| OS | Config files |
| --- | --- |
| Linux | `~/.config/trelloc/config.toml` and `~/.config/trelloc/board-access.toml` |
| macOS | `~/Library/Application Support/trelloc/config.toml` and `~/Library/Application Support/trelloc/board-access.toml` |
| Windows | `%APPDATA%\trelloc\config.toml` and `%APPDATA%\trelloc\board-access.toml` |

```toml
current_profile = "work"

[profiles.work]
api_key = "..."
token = "..."
default_board = "Ab12Cd34"     # used when --board is omitted
timezone = "Asia/Jakarta"

[profiles.work.aliases]
doing = "Roadmap/Doing"
```

Environment variables override the profile:

| Variable | Purpose |
| --- | --- |
| `TRELLO_API_KEY` | API key from your Trello app's **Trello Auth** settings |
| `TRELLO_TOKEN` | User token |
| `TRELLO_PROFILE` | Profile to use (or `--profile`) |
| `TRELLO_WORKSPACE` | Default workspace for board lookups |
| `TRELLO_DEFAULT_BOARD` | Board used when `--board` is omitted |
| `TRELLO_DEFAULT_LIST` | List used by `card create` when `--list` is omitted |
| `TRELLO_TIMEZONE` | IANA timezone for date input and display |
| `TRELLO_TIMEOUT_SECS` | HTTP timeout |
| `TRELLO_CARD_LIMIT` | Default cap for `card list` |

Global flags: `--profile`, `--workspace`, `--json`, `--refresh` (bypass caches), `-y/--yes`, `-v/--verbose`.

### Multiple accounts

Each profile is one Trello **member** (an API key + token). A token sees every workspace that member belongs to; `--workspace` only narrows board-name lookups.

```bash
trelloc --profile personal auth login       # add or update a profile
trelloc auth profile list
trelloc auth profile use                    # picker
trelloc --profile personal card list --mine
trelloc --workspace "Side Project" board list
```

For agents, `trelloc mcp install` lets you tick several profiles; each becomes its own server entry (`trelloc-work`, `trelloc-personal`) pinned to that account. Every tool also accepts optional `profile` and `workspace` arguments, and `trello_profile_list` shows what's available.

Exit codes: `0` ok · `1` error · `2` invalid input · `3` auth · `4` not found / unresolved name · `5` rate limited.

## Security

- The API key and token are sent only in the `Authorization: OAuth …` header — never in a URL — are redacted from every error and log line, and are never written into MCP client configs.
- Release binaries are built in GitHub Actions with build provenance attestations; crates.io and npm are published through OIDC trusted publishing — no long-lived publish tokens exist.
- Found a vulnerability? Please report it privately — see [SECURITY.md](SECURITY.md).

## Docs

- [Install and Trello auth guide](INSTALL.md) · [Contributing](CONTRIBUTING.md) · [Security policy](SECURITY.md) · [Code of conduct](CODE_OF_CONDUCT.md)
- [CLI spec](docs/CLI.md) · [MCP tools](docs/MCP.md) · [Architecture](docs/ARCHITECTURE.md) · [Trello API notes](docs/TRELLO-API.md) · [Roadmap](docs/ROADMAP.md)
- Rust library: [`trelloc-core`](https://docs.rs/trelloc-core) (client, resolver, models) and [`trelloc-mcp`](https://docs.rs/trelloc-mcp) (embeddable MCP server)

## License

[MIT](LICENSE). Trello is a trademark of Atlassian; this project is not affiliated with or endorsed by Atlassian.
