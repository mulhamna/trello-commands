# `trelloc` CLI spec

Conventions:
- Boards accept id, shortLink, URL, alias, or name. Lists accept name, id, or `Board/List`. Labels accept name or color. Members accept `me`, username, full name, or id. Custom fields accept name or id.
- Cards accept id, shortLink, card URL, `board#42`, or `#42` (on `default_board`).
- `--board` falls back to `default_board`; on a TTY a missing board/list/card opens a picker.
- Dates: `2026-10-20`, `2026-10-20 14:00`, `today`, `tomorrow`, `fri`, `+3d`, `eow`, `none` (clear).
- Global flags: `--profile <name>`, `--json`, `-v/--verbose`, `--refresh`, `-y/--yes`.
- Phase tags (P1…P5) refer to `docs/ROADMAP.md`.

---

## auth (P1)

```
trelloc auth login [--api-key K] [--token T] [--profile work]
    # 1. no key? point to trello.com/apps/admin → create an app without Power-Up capabilities → Trello Auth → Generate API key
    # 2. no token? open the authorize URL (expiration=never, scope=read,write,account, name=trelloc) and prompt for the token
    # 3. GET /1/members/me once → cache member_id/username in the profile
    # 4. optionally pick a default workspace and board
trelloc auth status                     # profile, member, key/token present (masked), config path, rate-limit headroom
trelloc auth logout [--profile work]
trelloc auth profile list | use <name> | remove <name>
trelloc whoami                          # from cache; --refresh hits /1/members/me
```

## hierarchy (P1)

```
trelloc workspace list
trelloc workspace use <name|id>
trelloc board list [--workspace <w>] [--closed] [--starred]
trelloc board view <board>              # lists (with card counts), labels, members, custom fields
trelloc board use <board>               # set default_board
trelloc tree [--workspace <w>]          # Workspace → Board → List
trelloc list list [--board <b>] [--closed]
trelloc alias set <name> <board|board/list> | list | remove <name>
trelloc member list [--board <b> | --workspace <w>]
trelloc cache clear
```

### Board access

```bash
trelloc board access list
trelloc board access all
trelloc board access activate <board>
trelloc board access deactivate <board>
trelloc board access select              # interactive board picker
```

Board access is stored per profile and applies to CLI and MCP. Legacy profiles allow all boards until setup; restricted profiles reject inactive board references and disable raw API commands.

Board/list management (P4):
```
trelloc board create <name> [--workspace <w>] [--from <board>] [--no-default-lists]
trelloc board rename <board> <name>
trelloc board archive|unarchive <board>
trelloc board delete <board> [--yes]
trelloc list create <name> [--board <b>] [--pos top|bottom|N]
trelloc list rename|archive|unarchive <list> [--board <b>]
trelloc list move-cards <list> --to <list|board/list> [--board <b>] [--yes]
trelloc list archive-cards <list> [--board <b>] [--yes]
```

## card (P2)

### list / search

```
trelloc card list [filters…] [--limit N] [--sort due|created|activity|pos] [--reverse]
```

| Flag | Repeatable | Behaviour |
| --- | --- | --- |
| `--board <b>` | yes | scope; board scan |
| `--list <l>` | yes | scope (needs a board, or `Board/List`) |
| `--member <m>` / `--mine` | yes | assigned member(s) |
| `--label <name\|color>` | yes | has label(s) |
| `--due-before <d>` / `--due-after <d>` | | due range |
| `--overdue` | | due < now and not complete |
| `--no-due` | | no due date |
| `--done` / `--not-done` | | `dueComplete` |
| `--created-after/before`, `--active-after/before` | | from id timestamp / `dateLastActivity` |
| `--has attachments\|description\|members\|cover` | yes | |
| `--field '<name><op><value>'` | yes | ops `=`, `!=`, `<`, `<=`, `>`, `>=`, `~` (contains), `empty`, `set` — evaluated client-side |
| `--text <s>` | | name/description contains |
| `--archived` | | include archived cards |
| `--query '<trello search>'` | | raw Trello search operators (`@me label:bug due:week`), forces search mode |

Mode is chosen automatically (see ARCHITECTURE "Filters"): board/list scope → exact board scan; `--query` or no scope → `/1/search` (prints a one-line note: "searched via Trello search; results may lag a few seconds").

Default columns: `Ref` (`board#idShort`), `Name`, `List`, `Members`, `Labels`, `Due`, `✓`.

Examples:
```
trelloc card list --board Roadmap --list Doing --mine
trelloc card list --mine --overdue
trelloc card list --board Roadmap --label bug --field 'Severity=High' --json
trelloc card list --query 'label:urgent -has:members'
```

### view

```
trelloc card view <card> [--comments] [--activity] [--web]
```
Name, refs (id, shortLink, `board#n`), URL, board/list, members, labels, start/due/done, custom fields, checklists with progress and items, attachments, description (Markdown), badges. `--comments` adds the latest comments; `--activity` the latest actions.

### create / update

```
trelloc card create <name> [--list <l>] [--board <b>] [--desc <md> | --desc-file <path|->]
                    [--pos top|bottom] [--member …]… [--label …]…
                    [--due <d>] [--start <d>] [--field 'Name=value']…
                    [--checklist 'Name:item1;item2']… [--from <card> [--keep all|checklists,labels,…]] [--open]

trelloc card update <card> [--name] [--desc | --desc-file] [--due <d>|none] [--start <d>|none]
                    [--done | --not-done] [--add-member …]… [--remove-member …]…
                    [--add-label …]… [--remove-label …]… [--field 'Name=value']… [--clear-field <name>]…
                    [--watch | --unwatch] [--cover <color|none>]
```

`--add-*`/`--remove-*` use the dedicated add/remove endpoints (never read-modify-write the whole set). `--field` is applied with separate PUTs and reported per field.

### move / copy / archive / delete

```
trelloc card move <card> --to <list|board/list> [--pos top|bottom]   # warns about labels/fields lost on cross-board moves
trelloc card copy <card> --to <list|board/list> [--name <n>] [--keep all|…]
trelloc card done <card> | undone <card>                              # dueComplete
trelloc card archive <card>… | unarchive <card>…
trelloc card delete <card>… [--yes]                                   # permanent
trelloc card open <card>
```

## collaboration (P3)

```
trelloc comment list <card> [--all]
trelloc comment add <card> <text | -> 
trelloc comment edit <card> <comment-id> <text>
trelloc comment delete <comment-id> [--yes]

trelloc checklist list <card>
trelloc checklist add <card> <name> [--item <text>]…
trelloc checklist item add <card> <checklist> <text> [--due <d>] [--member <m>]
trelloc checklist item check|uncheck <card> <item>       # item by id or text (fuzzy within the card)
trelloc checklist item delete <card> <item> [--yes]
trelloc checklist delete <card> <checklist> [--yes]

trelloc label list [--board <b>]
trelloc label create <name> --color <c> [--board <b>]
trelloc label edit <label> [--name] [--color] [--board <b>]
trelloc label delete <label> [--board <b>] [--yes]

trelloc field list [--board <b>]
trelloc field set <card> <field> <value>                 # dropdown option by name, dates, numbers, checkbox true/false
trelloc field clear <card> <field> [--yes]

trelloc attach <card> <file|url>… [--name <n>] [--cover]
trelloc attachment list <card>
trelloc attachment delete <card> <attachment-id> [--yes]
```

## bulk (P4)

```
trelloc card bulk move    [filters…] --to <list> [--dry-run] [--yes]
trelloc card bulk update  [filters…] [--add-label …] [--remove-label …] [--add-member …] [--remove-member …] [--due <d>] [--mark-done|--mark-not-done] [--dry-run] [--yes]   # --done/--not-done are filters
trelloc card bulk archive [filters…] [--dry-run] [--yes]
trelloc card bulk comment [filters…] --message <t> [--dry-run] [--yes]   # --text is the name/desc filter
trelloc card batch --file ops.json [--dry-run]           # [{ "op": "create"|"update"|"move"|"comment"|"archive"|"delete", … }]
```
Matched-count preview, confirmation above 1 card, bounded concurrency through the shared limiter, progress bar, per-card result summary.

## reports (P4)

```
trelloc standup [--since yesterday] [--board <b>]   # my cards: moved/completed/commented since, in progress, due soon, overdue
trelloc summary --board <b>                          # cards per list, per member, overdue, no-due, done
trelloc activity [--board <b>] [--since <d>] [--member <m>]
```

## escape hatch (P1)

```
trelloc api GET /members/me/boards --query 'fields=name,url'
trelloc api POST /cards --query 'idList=…&name=Hello'
trelloc api PUT /cards/{card} --data '{"closed":true}'
```
Paths are relative to `/1`. Credentials are added as the header. Non-GET asks for confirmation unless `--yes`.

## config / mcp (P1 / P5)

```
trelloc config get|set <key> [value]          # default_board, default_list, workspace, timezone, default_card_limit, timeout_secs
trelloc mcp serve [--transport stdio|streamable-http] [--host] [--port] [--path]
trelloc mcp doctor                            # binary on PATH, credentials, a test call, rate-limit headroom
trelloc mcp install [--client claude-code|claude-desktop|cursor|codex|gemini-cli|vscode|…]… [--server-profile <p>]… [--dry-run] [--print] [--force]
                                              # no --client: interactive picker; global --profile pins that profile
```
`mcp install` writes `command: "<path to trelloc>", args: ["mcp", "serve"]` (+ `env.TRELLO_PROFILE` when a profile is pinned; several profiles become `trelloc-<profile>` entries). Never writes the key or token into client configs.

## update

```
trelloc update [--check] [--yes] [--json]     # latest GitHub release; upgrade via npm / brew / scoop / cargo / install script
```
The channel is guessed from the binary's path. The upgrade command runs after a confirmation (`--yes` skips it); `--check` only reports. On Windows it prints the command, because a running `.exe` can't be replaced.
