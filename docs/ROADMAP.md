# Roadmap

Scope for v1: **core + CLI + MCP**, one binary. No TUI.

Work one phase at a time. A phase is done when its acceptance criteria pass **and** the smoke test is green:

```bash
cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all && cargo build --all
```

Commit per task with Conventional Commits (`feat(core): …`, `feat(cli): …`, `feat(mcp): …`). Stop and report to the repo owner at the end of each phase.

---

## Phase 0 — Scaffold

- [x] Cargo workspace: `crates/trelloc-core` (lib), `crates/trelloc-mcp` (lib, no binary), `crates/trelloc` (the only binary). Edition 2021, `license = "MIT"`, version `0.0.0` in `[workspace.package]` (release-please sets the real one) with `# x-release-please-version` markers, shared deps in `[workspace.dependencies]` (drop `chrono-tz` only if unused).
- [x] `LICENSE`, `.gitignore`, `README.md`, `CLAUDE.md`, `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`.
- [x] CI: `ci.yml`, `security.yml`, `deny.toml`, `dependabot.yml`, templates.
- [x] `trelloc --version` runs; `trelloc mcp serve` answers `initialize` over stdio with an empty tool list.

**Accept:** CI matrix green; exactly one binary produced.

## Phase 1 — Core foundation, auth, hierarchy

Core:
- [x] `config.rs` (profiles, env overrides, chmod-600 warning, cached member, save/load round-trip).
- [x] `error.rs`.
- [x] `client/mod.rs`: OAuth-style `Authorization` header (sensitive), `Query` builder, JSON + plain-text error parsing, redaction, `raw()`, `batch()` (chunks of 10).
- [x] `rate_limit.rs`: token bucket 90/10 s, members bucket 90/900 s, search bucket, `RouteClass`, header observation, 429 backoff ×4, shared `Arc`, semaphore.
- [x] `model.rs`: member, workspace, board, list, label, member-on-board, custom field definition.
- [x] `client/{member,board,list}.rs`: me, my workspaces, my boards, workspace boards/members, board snapshot, board lists/members/labels/fields.
- [x] `board_ref.rs` + tests.
- [x] `resolve.rs`: workspace, board (id/shortLink/URL/alias/name), snapshot cache, list (name/id/`Board/List`), label, member (`me` from cache), fuzzy suggestions, ambiguity error, TTL + refresh-on-miss.
- [x] `time.rs`: input grammar, ISO output, `created_at(id)`.

CLI:
- [x] `auth login|status|logout|profile`, `whoami`, `workspace list|use`, `board list|view|use`, `tree`, `list list`, `member list`, `alias`, `cache clear`, `config get|set`, `api`.
- [x] `--json` everywhere; exit codes.

MCP:
- [x] `trelloc-mcp/src/lib.rs` ported; `trelloc mcp serve` wired; stdout clean in stdio mode.
- [x] `error.rs` ported.
- [x] Tools: `trello_auth_status`, `trello_whoami`, `trello_profile_list`, `trello_workspace_list`, `trello_board_list`, `trello_board_get`, `trello_member_list`, `trello_api_request`.

**Accept:**
- wiremock: header is `OAuth oauth_consumer_key="…", oauth_token="…"`; **no request URL contains `key=` or `token=`**; 429 then 200 succeeds; 429 ×5 → `RateLimit` carrying the limit name; plain-text 401 → `Auth`.
- Limiter (paused tokio clock): 200 sequential requests never exceed 90 per rolling 10 s; `/members/me` draws from the members bucket, `/members/me/boards` doesn't.
- Resolver: id, shortLink, URL, alias, unique name, ambiguous name, typo suggestion; second `board view` hits cache (zero HTTP).
- Commands other than `auth login` and `whoami --refresh` make zero `/1/members/me` calls.
- Manual (owner): `trelloc auth login` end-to-end, `trelloc tree`.

## Phase 2 — Cards

Core:
- [x] `card_ref.rs` + exhaustive tests (id, shortLink, URL, `board#42`, `#42`, garbage).
- [x] `model.rs`: card (summary + full), custom field item display.
- [x] `filter.rs`: `CardFilter`, `plan()`, `matches()`, `to_search_query()`.
- [x] `client/card.rs`: get, board cards (open/closed/all), list cards, member cards, search, create, update, add/remove member, add/remove label, move (same/cross board), copy, archive/unarchive, delete, card by board number.
- [x] `ops.rs`: `search_cards`, `create_card`, `update_card`, `move_card` (cross-board warnings, label mapping by name), `copy_card`, `card_summary`.

CLI:
- [x] `card list|view|create|update|move|copy|done|undone|archive|unarchive|delete|open` per `docs/CLI.md`, pickers on TTY, delete confirmation.

MCP:
- [x] `trello_card_search`, `trello_card_get`, `trello_card_create`, `trello_card_update`, `trello_card_move`, `trello_card_archive`, `trello_card_delete`.

**Accept:**
- Filter planning: scoped → board scan (1 request per board, batched); `--query` → `/1/search` with expected operators; `--mine` unscoped → member cards.
- Predicate tests for every filter flag incl. overdue (excludes `dueComplete`), no-due, created-after via id timestamp.
- Update with only `--add-label` calls the add-label endpoint, never `PUT idLabels`.
- Cross-board move returns warnings for labels missing on the target board.
- MCP: `trello_card_delete` without `confirm` → `unsafe_operation`; `trello_card_archive` works without it.

## Phase 3 — Collaboration

Core + CLI + MCP:
- [x] Comments: list (paged), add, edit, delete.
- [x] Checklists: list, add (with items), item add/check/uncheck/rename/delete, delete checklist; item lookup by text.
- [x] Labels: list, create, update, delete.
- [x] Custom fields: list, set (text/number/date/checkbox/dropdown option by name), clear; `card create|update --field` wired; 403 → `Unavailable` with Power-Up hint.
- [x] Attachments: list, upload file (multipart), attach URL, delete.

**Accept:**
- Field value bodies match TRELLO-API §10 exactly (number and checkbox as strings).
- Long description/comment (5 000 chars) is not sent in the URL.
- Every **(verify)** endpoint used in this phase is checked and unmarked in `docs/TRELLO-API.md`.

## Phase 4 — Bulk, board/list management, reports

- [x] Bulk move/update/archive/comment + batch file (CLI) and `trello_card_bulk_create`, `trello_card_bulk_update`, `trello_batch` (MCP): dry-run, confirmation, bounded concurrency, progress, per-item results.
- [x] Board create/rename/archive/delete; list create/rename/archive, move-cards, archive-cards; `trello_board_create|update|delete`, `trello_list_create|update`.
- [x] Card copy (`trello_card_copy`).
- [x] Reports: `standup`, `summary`, `activity` + `trello_standup`, `trello_board_summary`, `trello_activity`.

**Accept:** bulk update of 300 mocked cards with a simulated 429 mid-way finishes with a correct summary and never exceeds 90 req / 10 s; dry-run makes zero mutating calls.

## Phase 5 — MCP install + polish

- [x] `trelloc mcp doctor` and `trelloc mcp install --client …`.
- [ ] Review every tool description by running the server in Claude Code against a real account (owner).
- [ ] README examples verified against a real board.

**Accept:** `trelloc mcp install --client claude-code` produces a working entry without key/token in it.

## Phase 6 — Release & distribution

Single-lane setup, named per `CLAUDE.md` "Names":
- [ ] Before the first release (owner): reserve/set up crates.io trusted publishing for `trelloc-core`, `trelloc-mcp`, `trelloc`; npm trusted publishing for `@mulham28/trelloc`; `release` environment; `RELEASE_PLEASE_TOKEN`, `PACKAGES_TOKEN` secrets.
- [x] release-please (one package, `/VERSION`, tag `vX.Y.Z`), `release.yml` (validate → build ×6 → GitHub release + checksums → crates → npm → brew/scoop), idempotent publish steps, `dry_run` input.
- [x] `packaging/npm` (`@mulham28/trelloc`, bin `trelloc`), `write_package_manifests.py` (`Formula/trelloc.rb`, `bucket/trelloc.json`), `install.sh`, `install.ps1`, `INSTALL.md`.
- [x] Add the "Release flow" section to `CLAUDE.md`.

**Accept:** `release.yml` with `dry_run: true` builds all artifacts and publishes nothing.

---

## Later (not v1)

- TUI kanban board (lists as columns — Trello's natural shape).
- Notifications (`/1/members/me/notifications`) and webhooks.
- Power-Up data, stickers, card covers beyond color.
- OAuth 1.0 app flow (only if distributing as a hosted app).
