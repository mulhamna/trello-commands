# CLAUDE.md — Rust Trello CLI + MCP

Guide for Claude (and contributors) when working in this repo.

When in doubt about *how* something is structured, read `docs/ARCHITECTURE.md`. When in doubt about *what* Trello does, read `docs/TRELLO-API.md`, then the official reference.

---

## Claude rules — MUST follow

Claude may:
- Create and edit files on the filesystem
- Run `cargo` commands for build/test/check
- Run `git` commands for read and write operations when explicitly requested by the repo owner, including `git add`, `git commit`, and `git push`
- Do not manually bump versions, edit generated changelogs, create/push tags, or rewrite git history unless explicitly requested by the repo owner

Claude must:
- Work phase by phase from `docs/ROADMAP.md`. Do not start a phase until the previous phase's acceptance criteria are green.
- Never call the real Trello API from tests. Use `wiremock` with `TRELLO_BASE_URL` pointed at the mock server.
- Never print, log, or commit the API key or token. Send credentials in the `Authorization` header, never in the URL, so they can't leak through logged URLs. Redact both in debug output.
- Verify any endpoint marked **(verify)** in `docs/TRELLO-API.md` against the official reference before implementing it, then remove the marker.

---

## Common commands

```bash
cargo build --all                                   # build workspace
cargo test --all                                    # run all tests
cargo test -p trelloc-core client::                 # tests matching a path in one crate
cargo run -p trelloc -- card list --board Roadmap   # run the CLI
cargo run -p trelloc -- mcp serve                   # run the MCP server (stdio)
cargo fmt --all                                     # format
cargo clippy --all-targets --all-features -- -D warnings  # lint
```

---

## Project overview

**One binary, `trelloc`**, that is both a CLI and an MCP server (`trelloc mcp serve`) for Trello, built on the Trello REST API (`https://api.trello.com/1`) with an **API key + token**.

Why it exists: Trello's official MCP server (`mcp.trello.com/v1`) is OAuth-only, sees one workspace per connection, cannot delete anything (archive only), and doesn't yet cover comments, attachments, custom fields, label management, board editing, card copying or member management (all listed as "planned" in its README). Going straight to the REST API gives token auth, every workspace the token can see, and the full API surface.

### Names (fixed — don't rename)

Brand is **`trelloc`** (mirrors `jirac`). Every published name uses it, so nothing collides with the existing `trello` crate or looks like an official Atlassian package.

| Thing | Name |
| --- | --- |
| GitHub repo | `mulhamna/trello-commands` |
| Library crate | `trelloc-core` |
| CLI crate + binary | `trelloc` |
| MCP crate (library only, linked into `trelloc`) | `trelloc-mcp` |
| npm | `@mulham28/trelloc` (one package, one binary) |
| Config dir | `~/.config/trelloc/` |
| Cache dir | `~/.cache/trelloc/` |
| Env prefix | `TRELLO_` (`TRELLO_API_KEY`, `TRELLO_TOKEN`, …) — what other Trello tools already use |

All package descriptions say "Unofficial" (e.g. "Unofficial Trello CLI and MCP server in one binary"). Trello and Atlassian are trademarks.

### Workspace structure

```
crates/
├── trelloc-core/               # PUBLIC LIBRARY (crates.io: "trelloc-core")
│   └── src/
│       ├── lib.rs
│       ├── client/             # TrelloClient — split by resource
│       │   ├── mod.rs          # http plumbing, auth header, retry, batch helper
│       │   ├── member.rs       # me, workspaces (organizations), members
│       │   ├── board.rs        # boards, board snapshot (lists/labels/members/fields in one call)
│       │   ├── list.rs         # lists
│       │   ├── card.rs         # card CRUD, move, copy, archive, search
│       │   ├── comment.rs      # comment actions
│       │   ├── checklist.rs
│       │   ├── label.rs
│       │   ├── field.rs        # custom fields
│       │   ├── attachment.rs
│       │   └── action.rs       # activity feed
│       ├── config.rs           # TrelloConfig loader (toml + env, multi-profile)
│       ├── error.rs            # TrelloError + Result alias
│       ├── rate_limit.rs       # client-side token bucket + x-rate-limit-* tracking + 429 backoff
│       ├── card_ref.rs         # parse card id / shortLink / card URL / board#number
│       ├── board_ref.rs        # parse board id / shortLink / board URL
│       ├── filter.rs           # CardFilter → client-side predicate or /1/search query
│       ├── resolve.rs          # name → id resolution (boards, lists, labels, members, fields) + cache
│       ├── ops.rs              # human-input operations shared by CLI and MCP
│       ├── bulk.rs             # bounded-concurrency fan-out
│       ├── time.rs             # human date input → ISO-8601, display helpers
│       ├── de.rs               # serde helpers
│       └── model.rs            # member, workspace, board, list, card, label, checklist, field, action, attachment
├── trelloc/                    # BINARY (crates.io: "trelloc", binary: trelloc)
│   └── src/
│       ├── main.rs
│       ├── output.rs           # table / json rendering
│       └── cli/                # clap commands: auth, hierarchy (workspace/board/list), card, collab
│                               #   (comment/checklist/label/field/attach), report, api, config, mcp
└── trelloc-mcp/                # MCP SERVER LIBRARY (crates.io: "trelloc-mcp", no binary)
    └── src/
        ├── lib.rs              # pub run_stdio / run_streamable_http
        ├── server.rs           # rmcp #[tool] definitions
        ├── models.rs           # tool I/O types (schemars)
        ├── error.rs            # AppError with stable codes
        └── app.rs              # thin adapters over trelloc-core::ops
```

No TUI in v1 (see ROADMAP "Later").

### Crate responsibilities

- **`trelloc-core`** — everything that talks to Trello. No `println!`, no clap, no rmcp. Returns typed models and `TrelloError`. `ops.rs` holds the "human input → API calls" logic so the CLI and MCP behave identically.
- **`trelloc/`** — clap parsing, interactive prompts (`inquire`) for missing args and destructive confirmation, table/JSON output. Binary: `trelloc`. Includes `trelloc mcp serve|doctor|install` (registration via `kurir`).
- **`trelloc-mcp/`** — rmcp server exposing `trelloc-core` as tools, as a **library**. Every tool name is prefixed `trello_`. No `main.rs`; the `trelloc` binary calls it from `trelloc mcp serve`.

### Single binary, single release

- One binary: `trelloc`. MCP lives under `trelloc mcp serve|doctor|install`.
- One version for the whole workspace (`/VERSION`), one tag lane `vX.Y.Z`, one GitHub Release with one archive per platform, one npm package.
- `trelloc-mcp` stays a separate crate only for code organisation and testing; it's published to crates.io solely because `trelloc` depends on it.
- MCP client configs point at `trelloc` with args `["mcp", "serve"]`.

Don't add a second `[[bin]]`, a second VERSION file, or a second release-please component.

---

## Trello API — implementation rules

Full notes: `docs/TRELLO-API.md`. The rules below are the ones that bite.

- **Auth**: API key + user token. Send `Authorization: OAuth oauth_consumer_key="{key}", oauth_token="{token}"`. **Never** append `key=`/`token=` to URLs (they'd end up in logs and error messages).
- **Base URL**: `https://api.trello.com/1`. Overridable via `TRELLO_BASE_URL` (tests). Never build URLs in the CLI or MCP crates.
- **Workspace = organization**: the API calls it `organization` / `idOrganization`. Use `workspace` in our code and UX; translate at the HTTP layer.
- **Rate limits** (all in 10-second windows): **100 req/10s per token**, 300 req/10s per API key. `/1/members/*` routes have an extra limit of **100 req per 900s** (nested routes like `/1/members/me/boards` are exempt, `/1/members/me` itself is not). `/1/search` and `/1/membersSearch` also have stricter limits.
  - Pace client-side with a token bucket (default 90 per 10s) shared by all callers; don't wait for 429s.
  - Read `x-rate-limit-api-token-remaining` / `-interval-ms` and `x-rate-limit-api-key-*` from every response.
  - On 429: there is no reset header. Back off (1s, 2s, 4s, capped at the 10s window), max 4 retries, then `TrelloError::RateLimit`. The body names the limit (`API_TOKEN_LIMIT_EXCEEDED`, `API_KEY_LIMIT_EXCEEDED`, `API_TOKEN_DB_LIMIT_EXCEEDED`) — keep it in the error.
  - **Cache `me`** (id, username, fullName) in the profile at login. Calling `GET /1/members/me` per command would burn the 100/15min members budget.
- **Prefer one fat call over many thin ones**: `GET /1/boards/{id}?lists=open&labels=all&members=all&customFields=true` returns a board's whole schema; `GET /1/boards/{id}/cards?customFieldItems=true` returns every open card. Use `GET /1/batch` (GET-only, up to 10 URLs — verify) when several independent reads are needed.
- **Dates are ISO-8601 strings** (`due`, `start`, `dateLastActivity`), not unix ms. Card creation time is derived from the card id (first 8 hex chars = unix seconds).
- **Positions**: `pos` is `top`, `bottom`, or a positive float.
- **Paging**: card and action collections cap at 1000 per request; page with `before`/`since` (an id or ISO date). Card actions (`/1/cards/{id}/actions`) use `page` (max 19). Search uses `cards_limit` + `cards_page`.
- **Card references**: a card has a 24-hex `id`, an 8-char `shortLink` (works anywhere an id does), and a per-board number `idShort` (`#42`). `card_ref.rs` turns ids, shortLinks, URLs (`trello.com/c/{shortLink}/…`) and `board#42` into an API id — callers never do.
- **Descriptions** are Markdown natively (`desc`). Comments too. No conversion.
- **Archive ≠ delete**: `closed=true` archives (reversible); `DELETE` is permanent. Default UX is archive. Delete is a separate command/tool and is destructive.
- **Labels, members and custom fields are board-scoped.** Moving a card to another board can drop labels/fields that don't exist there — warn before cross-board moves.
- **Custom fields** need the Custom Fields Power-Up on the board (and a paid plan — verify); without it reads return `[]` and writes return 403. Set values with `PUT /1/cards/{idCard}/customField/{idField}/item`; clear by sending empty `value`/`idValue`. Number values are sent as **strings**.
- **Destructive ops** (delete card/list content/board, delete comment, delete checklist/item, remove label/member, clear field): CLI asks for confirmation unless `--yes`; MCP requires `confirm: true`.

---

## Smoke test

Claude runs this before reporting to the repo owner. Fix until all green.

```bash
cargo fmt --all -- --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test --all && \
cargo build --all
```

---

## Conventional Commits

Format: `<type>(<scope>): <description>` — in English. Scopes: `core`, `cli`, `mcp`, `docs`, `ci`.

| Type                              | Bump  |
| --------------------------------- | ----- |
| `feat:`                           | MINOR |
| `fix:`, `perf:`, `refactor:`      | PATCH |
| `feat!:` / `BREAKING CHANGE:`     | MAJOR |
| `chore:`, `docs:`, `ci:`, `test:` | none  |

### Release flow (single lane)

```
feature PRs (conventional commits) ─► main
        │
        ▼  maintainer runs "release-please" (workflow_dispatch)
release PR: VERSION, Cargo.toml (x-release-please-version), packaging/npm/package.json,
            CHANGELOG.md, Cargo.lock (re-synced by the workflow)
        │
        ▼  merge release PR → release-please tags vX.Y.Z + creates the GitHub Release
release.yml (tag push):
  validate-tag ─► build ×6 (+ provenance) ─► github-release (archives + checksums.txt)
                                               ├─► publish-crates  (OIDC: core → mcp → trelloc)
                                               ├─► publish-npm     (OIDC, @mulham28/trelloc)
                                               └─► update-packages (brew tap, scoop bucket)
```

- Never bump versions, edit `CHANGELOG.md`, or push tags by hand; release-please owns all of it.
- crates.io and npm publish with **OIDC trusted publishing** from the `release` environment. There are no publish tokens in secrets. Repo secrets are PATs only: `RELEASE_PLEASE_TOKEN` (the tag push must trigger workflows) and `PACKAGES_TOKEN` (push to the Homebrew tap and Scoop bucket).
- Before the first release the owner sets up trusted publishing for `trelloc-core`, `trelloc-mcp`, `trelloc` and `@mulham28/trelloc`, plus the `release` environment.
- Every publish step is idempotent, so a failed release can be re-run from the Actions tab (`workflow_dispatch` with the tag and `dry_run: false`). `dry_run: true` builds everything and publishes nothing.
- Don't add a second release-please package, a second `VERSION`, or a second tag pattern.

---

## Docs index

| File | What |
| --- | --- |
| `docs/ROADMAP.md` | Phases, tasks, acceptance criteria — **start here** |
| `docs/ARCHITECTURE.md` | Module design, key types, data flow |
| `docs/TRELLO-API.md` | Endpoints, payloads, limits, gotchas |
| `docs/CLI.md` | `trelloc` command spec |
| `docs/MCP.md` | MCP server (`trelloc mcp serve`) tool spec |
| `docs/ONBOARDING.md` | Prompts the repo owner uses to drive each phase |
