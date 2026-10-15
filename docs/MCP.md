# MCP server tool spec

Run: `trelloc mcp serve [--transport stdio|streamable-http] [--host 127.0.0.1] [--port 8787] [--path /mcp]`. No separate MCP binary; the code lives in the `trelloc-mcp` library crate and ships inside `trelloc`.

Auth: same config/profile as the CLI (`~/.config/trelloc/config.toml`, `TRELLO_API_KEY`, `TRELLO_TOKEN`, `TRELLO_PROFILE`). No OAuth.

## Accounts

Every tool takes an optional `profile`. Omitted → the server's profile (`TRELLO_PROFILE` in the client entry, else the active profile). `trello_profile_list` lists profiles. Rate limiting is tracked per token. Unlike the official server, there's no one-workspace-per-connection limit: tools see every workspace and board the token can see, and accept an optional `workspace` to narrow board resolution.

## Design rules

1. **Names in, ids out.** Boards, lists, labels, members, custom fields and dropdown options accept names; outputs always include ids and `ref` (`board#42`) so the model can chain calls.
2. **Compact by default.** Card summary: `id`, `ref`, `short_url`, `name`, `board {id, name}`, `list {id, name}`, `members [{id, username}]`, `labels [{id, name, color}]`, `due`, `due_complete`, `closed`. `detail: "full"` adds description, custom fields, checklists, attachments, badges.
3. **Human dates** in (`tomorrow`, `+3d`, ISO); ISO-8601 out, in the configured timezone.
4. **Archive is the safe default.** `trello_card_archive` needs no confirmation (reversible). Every permanent delete/remove/clear needs `confirm: true` (`unsafe_operation` otherwise). Bulk tools need `confirm: true` above 1 card; `dry_run: true` returns the plan.
5. **Stable errors** (`AppError`): `validation_error`, `unsafe_operation`, `auth_missing`, `config_error`, `resolve_error` (`data.suggestions`), `trello_api_error` (`data.status`, `data.code`), `rate_limited` (`data.limit`), `unavailable` (e.g. Custom Fields Power-Up off — `data.hint`). 4xx → `INVALID_PARAMS`, 5xx → `INTERNAL_ERROR`.
6. **Descriptions are prompts.** Say when to use each tool and the traps, e.g. "prefer trello_card_search with a board for exact results; `query` uses Trello search syntax and may lag", "moving to another board can drop labels — check warnings".
7. Rate limiting is invisible to the model; only exhausted retries surface as `rate_limited`.

## Tools

Phase tags refer to `docs/ROADMAP.md`. "Official" = what Trello's official MCP (`mcp.trello.com/v1`) offers per its README, Oct 2026: ✓ available, ◐ planned there, ✗ not offered.

### Auth & meta (P1)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_auth_status` | — | profile, member, credentials present (masked), rate-limit headroom | — |
| `trello_whoami` | — | cached member (id, username, fullName) | ✓ |
| `trello_profile_list` | — | | — |

### Hierarchy (P1)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_workspace_list` | — | all workspaces | ◐ (one per connection) |
| `trello_board_list` | `workspace?`, `include_closed?`, `starred?` | | ✓ |
| `trello_board_get` | `board` | lists (with card counts), labels, members, custom fields — one snapshot call | ✓ |
| `trello_member_list` | `board?` \| `workspace?` | | ◐ |
| `trello_board_create` | `name`, `workspace?`, `from_board?`, `default_lists?` | (P4) | ✓ |
| `trello_board_update` | `board`, `name?`, `desc?`, `closed?` | (P4) | ◐ |
| `trello_board_delete` | `board`, `confirm` | (P4) | ✗ |
| `trello_list_create` | `board`, `name`, `pos?` | (P4) | — |
| `trello_list_update` | `board`, `list`, `name?`, `pos?`, `closed?` | (P4) | ✓ (move) |

### Cards (P2)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_card_search` | `boards?`, `lists?`, `members?`, `labels?`, `due_before?`, `due_after?`, `overdue?`, `no_due?`, `due_complete?`, `created_after?`, `active_after?`, `has?`, `fields?: [{field, op, value}]`, `text?`, `query?` (Trello syntax), `include_archived?`, `sort?`, `limit?` (default 50, max 1000), `detail?` | board scan when scoped, `/1/search` otherwise; response says which | ✓ (keyword) |
| `trello_card_get` | `card`, `detail?`, `include_comments?`, `include_activity?` | `card` = id, shortLink, URL, `board#42` | ✓ |
| `trello_card_create` | `list` (or `board` + `list`), `name`, `desc?` (markdown), `pos?`, `members?`, `labels?`, `due?`, `start?`, `custom_fields?: {name: value}`, `checklists?: [{name, items}]`, `copy_from?` | | ✓ |
| `trello_card_update` | `card`, `name?`, `desc?`, `due?` (`null` clears), `start?`, `due_complete?`, `members_add?`, `members_remove?`, `labels_add?`, `labels_remove?`, `custom_fields?`, `subscribed?` | add/remove endpoints, fields via per-field PUT | ✓ (fields ◐) |
| `trello_card_move` | `card`, `list` (name or `Board/List`), `pos?` | returns `warnings` for labels/members/fields lost cross-board | ✓ |
| `trello_card_copy` | `card`, `list`, `name?`, `keep?` | (P4) | ◐ |
| `trello_card_archive` | `card`, `unarchive?` | | ✓ |
| `trello_card_delete` | `card`, `confirm` | permanent | ✗ |

### Bulk (P4)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_card_bulk_create` | `list`, `cards: [create input]`, `dry_run?` | | — |
| `trello_card_bulk_update` | `cards: [ref]` \| `filter: {search input}`, `changes: {list?, labels_add?, labels_remove?, members_add?, due?, due_complete?, archive?}`, `dry_run?`, `confirm?` | | — |
| `trello_batch` | `ops: [{op, …}]`, `dry_run?`, `confirm?` | mixed create/update/move/comment/archive/delete | — |

### Comments (P3)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_comment_list` | `card`, `limit?` | | ◐ |
| `trello_comment_add` | `card`, `text` | markdown, `@username` mentions | ◐ |
| `trello_comment_edit` | `card`, `comment_id`, `text` | | ◐ |
| `trello_comment_delete` | `comment_id`, `confirm` | | ✗ |

### Checklists (P3)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_checklist_list` | `card` | with progress | ✓ |
| `trello_checklist_add` | `card`, `name`, `items?` | | ✓ |
| `trello_checklist_item_add` | `card`, `checklist`, `text`, `due?`, `member?` | | ✓ |
| `trello_checklist_item_update` | `card`, `item` (id or text), `checked?`, `text?`, `due?` | | ✓ |
| `trello_checklist_item_delete` / `trello_checklist_delete` | … + `confirm` | | ✗ |

### Labels, custom fields, attachments (P3)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_label_list` | `board` | | ✓ |
| `trello_label_create` / `trello_label_update` | `board`, `name`, `color` | | ◐ |
| `trello_label_delete` | `board`, `label`, `confirm` | | ✗ |
| `trello_field_list` | `board` | name, type, options; `unavailable` if Power-Up off | ◐ |
| `trello_field_set` | `card`, `field`, `value` | option name / date / number / bool | ◐ |
| `trello_field_clear` | `card`, `field`, `confirm` | | ✗ |
| `trello_attachment_list` | `card` | | ◐ |
| `trello_attachment_add` | `card`, `path?` \| `url?`, `name?`, `set_cover?` | local file upload | ◐ |
| `trello_attachment_delete` | `card`, `attachment_id`, `confirm` | | ✗ |

### Reports (P4)

| Tool | Input | Notes | Official |
| --- | --- | --- | --- |
| `trello_standup` | `since?`, `board?`, `member?` | | — |
| `trello_board_summary` | `board` | cards per list / member, overdue, no due, done | — |
| `trello_activity` | `board?`, `card?`, `since?`, `member?`, `limit?` | | ◐ |

### Escape hatch (P1)

| Tool | Input | Notes |
| --- | --- | --- |
| `trello_api_request` | `method`, `path` (relative to `/1`), `query?`, `body?`, `confirm?` | non-GET requires `confirm: true` |

## Not mirrored from the official server

- **Planner** (calendar events, focus time) and **Inbox** — not in the Trello REST OpenAPI spec (checked Oct 2026); revisit if Atlassian documents them.

## Client registration

```json
{
  "mcpServers": {
    "trello": {
      "command": "trelloc",
      "args": ["mcp", "serve"],
      "env": { "TRELLO_PROFILE": "work" }
    }
  }
}
```

Never write the API key or token into client config files.
