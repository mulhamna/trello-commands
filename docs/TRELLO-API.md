# Trello REST API notes

Working reference for implementers. Source of truth is <https://developer.atlassian.com/cloud/trello/> (REST reference under `/rest/`, guides under `/guides/rest-api/`, OpenAPI spec at `https://developer.atlassian.com/cloud/trello/swagger.v3.json`).

Legend: items without a marker were checked against the official guides and the OpenAPI spec (`swagger.v3.json`, Oct 2026). **(unverified)** = not in the spec or guides; trelloc relies on it, so confirm against a real account and update this doc.

---

## 1. Basics

| Item | Value |
| --- | --- |
| Base URL | `https://api.trello.com/1` |
| Credentials | API key (per Power-Up) + user token |
| Auth header | `Authorization: OAuth oauth_consumer_key="{key}", oauth_token="{token}"` |
| Other accepted forms | `?key=…&token=…` query params, or `key`/`token` in a PUT/POST body — **don't use them** |
| Params | The spec documents write params as **query parameters**. trelloc sends them as a JSON body instead so long text never lands in a URL — Trello reads body params (it documents `key`/`token` in bodies) **(unverified per endpoint)** |
| Ids | 24-hex MongoDB-style; always returned even when `fields=` restricts output |
| `fields=` | comma-separated attribute list or `all` on most GETs |

The first 8 hex characters of any id are its creation time in unix seconds — use this for "created" sorting/filtering of cards without extra calls.

## 2. Auth flow

1. **API key.** Created per Power-Up: <https://trello.com/power-ups/admin> → New (any workspace) → *API key* tab → Generate. Users of trelloc create their own (we never ship a shared key).
2. **Token.** Open
   `https://trello.com/1/authorize?expiration=never&scope=read,write,account&response_type=token&key={key}&name=trelloc`
   in the browser → Allow → the token is displayed. User pastes it back.
   - `scope`: `read`, `write`, `account` (needed for the user's email).
   - `expiration`: `1hour`, `1day`, `30days`, `never`.
   - `name`: app name shown on the consent page **(unverified — not in the auth guide, but widely used)**.
3. **Validate**: `GET /1/members/me?fields=id,username,fullName,email,initials` (counts against the members limit — do it once, then cache in the profile).
4. A revoked token → `401` with body `invalid token`.

OAuth 1.0 with the app secret also exists — out of scope for v1.

## 3. Rate limits

| Scope | Limit |
| --- | --- |
| Per token | 100 requests / 10 s |
| Per API key | 300 requests / 10 s |
| `/1/members/*` (top-level member routes) | 100 requests / 900 s — in addition to the above |
| `/1/search`, `/1/membersSearch` | stricter limits (values not published) |

- Nested routes under `/1/members` (e.g. `/1/members/me/boards`) do **not** count against the members limit; `/1/members/me` itself does.
- To list many members use `/1/boards/{id}/members` or `/1/organizations/{id}/members`.
- Response headers on every call: `x-rate-limit-api-token-interval-ms`, `x-rate-limit-api-token-max`, `x-rate-limit-api-token-remaining`, and the same with `api-key`.
- On exceed: `429`, body names the limit: `API_KEY_LIMIT_EXCEEDED`, `API_TOKEN_LIMIT_EXCEEDED`. More than 200 429s for a key → every request for that key is 429'd for the rest of the window.
- Other "back off" errors: `API_TOKEN_DB_LIMIT_EXCEEDED` (too many expensive DB calls — slow down, split requests), `API_TOO_MANY_CARDS_REQUESTED` (e.g. `cards?actions=all` — fetch actions separately).

Client strategy (`rate_limit.rs`):
1. Token bucket, 90 permits / 10 s (configurable), shared via `Arc` by every caller of the client.
2. Separate small bucket for top-level `/1/members/*` (90 / 900 s) and a conservative one for `/1/search` (e.g. 10 / 10 s, tune after testing).
3. Update from `x-rate-limit-*-remaining`; if remaining ≤ 2, wait for the interval.
4. On 429: exponential backoff 1 s → 2 s → 4 s → 8 s (cap 10 s), then `TrelloError::RateLimit { limit }`.
5. Bulk ops: bounded concurrency (default 4).

Official MCP (`mcp.trello.com/v1`) doesn't document its own limits.

## 4. Errors

| Code | Meaning |
| --- | --- |
| 400 | missing/invalid params (body is often plain text, e.g. `invalid value for idList`) |
| 401 | bad/missing credentials, or no permission (`invalid token`, `unauthorized permission requested`) |
| 403 | refused, e.g. over an object limit or Custom Fields Power-Up disabled |
| 404 | route or resource not found (`The requested resource was not found.`) |
| 409 | conflict with current state |
| 429 | rate limited |
| 449 | sub-request failed (batch) |
| 500 / 503 / 504 | server; 504 = GET took > 30 s |

Bodies are **plain text or JSON** (`{"message": "...", "error": "..."}`) depending on the route **(unverified which routes use which)**. Parse both: if JSON, take `message`/`error`; else the trimmed text. → `TrelloError::Api { status, code: Option<String>, message }`.

## 5. Object limits (read `limits` from the API at runtime; these are typical)

Board: 5000 open cards, 475 open lists, 950 labels, 1520 members. Card: 950 attachments, 475 checklists. Custom fields: 50 per board. Over the limit → API error / `maxExceeded`.

## 6. Hierarchy

```
Member (me) ─┬─ Workspace (organization) ─ Board ─ List ─ Card ─┬─ Checklist ─ CheckItem
             └─ Boards not in a workspace (personal)             ├─ Comment (action)
                                                                  ├─ Attachment
                                                                  ├─ Labels, Members (board-scoped)
                                                                  └─ Custom field items (board-scoped definitions)
```

| Operation | Method + path |
| --- | --- |
| Me | `GET /1/members/me` (members limit!) |
| My workspaces | `GET /1/members/me/organizations?fields=id,name,displayName,url` |
| My boards | `GET /1/members/me/boards?filter=open&fields=id,name,shortLink,url,idOrganization,closed,starred` |
| Workspace boards | `GET /1/organizations/{id}/boards?filter=open` |
| Workspace members | `GET /1/organizations/{id}/members` |
| Board snapshot | `GET /1/boards/{id}?fields=name,url,shortLink,idOrganization,prefs&lists=open&list_fields=name,pos,closed&labels=all&label_fields=name,color&labels_limit=1000&members=all&member_fields=fullName,username&customFields=true` |
| Board lists | `GET /1/boards/{id}/lists?filter=open|closed|all` |
| Board members | `GET /1/boards/{id}/members` |
| Board labels | `GET /1/boards/{id}/labels?limit=1000` |
| Board custom fields | `GET /1/boards/{id}/customFields` |
| Create board | `POST /1/boards/?name=…&idOrganization=…&defaultLists=false&idBoardSource=…` |
| Update / archive board | `PUT /1/boards/{id}` with `name`, `desc`, `closed`, `idOrganization`, `prefs/*` |
| Delete board | `DELETE /1/boards/{id}` |
| Create list | `POST /1/boards/{id}/lists` (`name`*, `pos`) or `POST /1/lists` (`name`*, `idBoard`*, `idListSource`, `pos`) |
| Update / archive list | `PUT /1/lists/{id}` (`name`, `closed`, `idBoard`, `pos`, `subscribed`); also `PUT /1/lists/{id}/closed?value=` |
| Move all cards in list | `POST /1/lists/{id}/moveAllCards` (`idBoard`*, `idList`*) |
| Archive all cards in list | `POST /1/lists/{id}/archiveAllCards` |

The board snapshot is the resolver's main cache entry: one call gives lists, labels, members and custom fields for a board.

## 7. Cards

### 7.1 Read

| Operation | Method + path |
| --- | --- |
| Card | `GET /1/cards/{id-or-shortLink}?fields=all&checklists=all&attachments=true&members=true&customFieldItems=true&list=true&board=true` |
| Single field | `GET /1/cards/{id}/{field}` |
| Board cards | `GET /1/boards/{id}/cards?customFieldItems=true` (open cards; `/cards/{filter}` with `all`, `closed`, `open`, `visible`) |
| Card by board number | not in the spec. trelloc reads `GET /1/boards/{id}/cards/all?fields=id,idShort` (paged) and matches `idShort` |
| List cards | `GET /1/lists/{id}/cards` |
| Card actions | `GET /1/cards/{id}/actions?filter=commentCard,updateCard:idList&page=0` (page max 19) |

Useful card fields: `id`, `idShort`, `shortLink`, `shortUrl`, `url`, `name`, `desc`, `closed`, `idBoard`, `idList`, `idMembers`, `idLabels`, `labels`, `pos`, `due`, `start`, `dueComplete`, `dueReminder`, `dateLastActivity`, `badges` (comments, attachments, checkItems counts), `cover`, `subscribed`, `isTemplate`, `cardRole`.

### 7.2 Create — `POST /1/cards`

Query params: `idList` (**required**), `name`, `desc`, `pos` (`top`/`bottom`/float), `due`, `start`, `dueComplete`, `idMembers` (comma-separated), `idLabels` (comma-separated), `urlSource`, `fileSource` + `mimeType`, `idCardSource` + `keepFromSource` (copy a card: `all` or e.g. `checklists,labels,attachments`), `address`, `locationName`, `coordinates`, `cardRole`.

### 7.3 Update — `PUT /1/cards/{id}`

Params: `name`, `desc`, `closed` (archive/unarchive), `idMembers`, `idAttachmentCover`, `idList`, `idLabels`, `idBoard`, `pos`, `due` (or `null`), `start` (or `null`), `dueComplete`, `subscribed`, `address`, `locationName`, `coordinates`, `cover` (`{color, brightness, url}`).

- `idMembers` / `idLabels` on update **replace** the whole set. For add/remove use the dedicated endpoints (§7.5) — they avoid read-modify-write races.
- Moving: set `idList` (same board) or `idBoard` + `idList` (cross-board) and optionally `pos`.

### 7.4 Delete — `DELETE /1/cards/{id}` (permanent).

### 7.5 Card relations

| Operation | Method + path |
| --- | --- |
| Add member | `POST /1/cards/{id}/idMembers` (`value` = member id) |
| Remove member | `DELETE /1/cards/{id}/idMembers/{idMember}` |
| Add label | `POST /1/cards/{id}/idLabels` (`value` = label id) |
| Remove label | `DELETE /1/cards/{id}/idLabels/{idLabel}` |
| Create label on board | `POST /1/labels` (`name`*, `color`*, `idBoard`*) |
| Update / delete label | `PUT /1/labels/{id}` (`name`, `color`), `DELETE /1/labels/{id}` |
| Watch | `PUT /1/cards/{id}?subscribed=true` |

Label colors: `green`, `yellow`, `orange`, `red`, `purple`, `blue`, `sky`, `lime`, `pink`, `black`, plus `_dark`/`_light` variants, or `null` (the spec's `Color` schema; trelloc validates against this set).

## 8. Search — `GET /1/search`

Params: `query` (required), `idBoards` (`mine` or comma ids), `idOrganizations`, `idCards`, `modelTypes` (`cards,boards,…`), `card_fields`, `cards_limit` (max 1000, default 10), `cards_page` (max 100), `card_board`, `card_list`, `card_members`, `card_attachments`, `board_fields`, `boards_limit`, `partial` (prefix matching).

The `query` supports Trello's search operators:

| Operator | Meaning |
| --- | --- |
| `@name`, `member:name`, `@me` | cards assigned to a member |
| `board:name` / `board:id` | in a board |
| `list:name` | in a list |
| `label:name`, `label:color`, `#name`, `#color` | has label |
| `due:day|week|month|overdue`, `due:N` | due within (next N days) |
| `due:complete`, `due:incomplete` | due-complete flag |
| `created:day|week|month|N`, `edited:…` | created / edited within last N days |
| `has:attachments|description|cover|members|stickers` | has … |
| `is:open`, `is:starred` | open cards / on starred boards |
| `name:`, `description:`, `checklist:`, `comment:` | text in that field |
| `sort:created|due|edited`, `sort:-edited` | ordering |
| `-` prefix | negation (`-has:members`) |

Search is eventually consistent (new cards can take a moment to appear) and has stricter rate limits — see §11 for when to use it.

## 9. Comments (actions)

| Operation | Method + path |
| --- | --- |
| List | `GET /1/cards/{id}/actions?filter=commentCard&page=N` |
| Add | `POST /1/cards/{id}/actions/comments?text=…` |
| Edit | `PUT /1/cards/{id}/actions/{idAction}/comments` (`text`*) — trelloc uses this; `PUT /1/actions/{id}/text` (`value`*) also exists |
| Delete | `DELETE /1/actions/{id}` — only comment actions can be deleted |

Comment text is Markdown; `@username` mentions notify. Long comments: trelloc sends `text` in the JSON body, never the query.

## 10. Checklists, attachments, custom fields

| Operation | Method + path |
| --- | --- |
| Card checklists | `GET /1/cards/{id}/checklists` |
| Create checklist | `POST /1/cards/{id}/checklists` (`name`, `idChecklistSource`, `pos`) or `POST /1/checklists` (`idCard`*) |
| Delete checklist | `DELETE /1/checklists/{id}` |
| Add item | `POST /1/checklists/{id}/checkItems` (`name`*, `pos`, `checked`, `due`, `dueReminder`, `idMember`) |
| Check / uncheck / rename item | `PUT /1/cards/{id}/checkItem/{idCheckItem}` (`state` = `complete`/`incomplete`, `name`, `idChecklist`, `pos`, `due`, `idMember`) |
| Delete item | `DELETE /1/checklists/{id}/checkItems/{idCheckItem}` |
| Attachments | `GET /1/cards/{id}/attachments` |
| Upload file | `POST /1/cards/{id}/attachments` multipart field `file`, optional `name` (≤ 256), `mimeType`, `setCover` |
| Attach URL | `POST /1/cards/{id}/attachments?url=…&name=…` |
| Delete attachment | `DELETE /1/cards/{id}/attachments/{idAttachment}` |
| Field definitions | `GET /1/boards/{id}/customFields` |
| Field values on cards | `customFieldItems=true` on card or board-cards GETs |
| Set value | `PUT /1/cards/{idCard}/customField/{idField}/item` |
| Create field | `POST /1/customFields` `{ idModel: boardId, modelType: "board", name, type, options?, pos }` |
| Add dropdown option | `POST /1/customField/{id}/options` |

Custom field value bodies:

| Type | Body |
| --- | --- |
| `text` | `{"value": {"text": "…"}}` |
| `number` | `{"value": {"number": "42"}}` (string!) |
| `date` | `{"value": {"date": "2026-10-20T09:00:00.000Z"}}` |
| `checkbox` | `{"value": {"checked": "true"}}` (string!) |
| `list` (dropdown) | `{"idValue": "{optionId}"}` |
| clear (any) | `{"value": ""}` / `{"idValue": ""}` — values can't be DELETEd |

Requires the Custom Fields Power-Up enabled on the board: reads return `[]`, writes `403` otherwise. Plan availability depends on Trello pricing **(unverified)**; trelloc maps the 403 to `Unavailable` with a Power-Up hint.

Attachment size limit: 10 MB on Free, 250 MB on paid plans **(unverified)**.

## 11. Choosing how to list cards

| Need | Use | Why |
| --- | --- | --- |
| Cards on one board (any filters) | `GET /1/boards/{id}/cards?customFieldItems=true` + client-side filter | 1 call, exact, no search limits, sees fresh cards |
| Cards in one list | `GET /1/lists/{id}/cards` | 1 call |
| Several boards | `GET /1/batch` of board-cards URLs (≤ 10 per batch), then filter | still exact |
| "My cards everywhere" | `GET /1/members/me/cards?filter=open` (`filter`: all, closed, complete, incomplete, none, open, visible; nested → not members-limited) | 1 call |
| Free-text across workspaces, or the user typed operators | `GET /1/search` | only option for text search |

Closed cards: `/cards/closed` or `/cards/all` — can be large; respect `limit` and page with `before` (id).

## 12. Batch — `GET /1/batch?urls=/boards/a/cards,/boards/b/cards`

GET-only, up to 10 URLs per call, comma-separated, each starting with `/` and without the version (spec). The per-item response shape — `{ "200": body }` on success, an error keyed by its status (or carrying `statusCode`) otherwise — is **(unverified)**; trelloc parses all three forms. Whether a batch counts as 1 or N against the rate limit is undocumented — budget it as N.

## 13. Activity

`GET /1/boards/{id}/actions?filter=…&since=…&before=…&limit=…` (limit ≤ 1000), `GET /1/members/{id}/actions` (`filter`; trelloc also passes `since`/`limit`; nested → not members-limited). Used for `standup`.

## 14. Not used in v1

Webhooks, Power-Up/plugin data, Butler/automation, enterprise admin, notifications (consider for "Later"), stickers, OAuth 1.0 app flow, Planner/Inbox (official MCP has these; they are not in the REST spec).
