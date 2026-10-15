# Onboarding Claude Code

## 1. Prepare

```bash
gh repo create mulhamna/trello-commands --public --clone
cd trello-commands
# copy in: README.md, CLAUDE.md, AGENTS.md, docs/*
git add . && git commit -m "docs: add design docs and agent guide" && git push -u origin main
```

For manual testing later, get your own credentials (don't paste them into Claude):
1. <https://trello.com/power-ups/admin> → New → any workspace → **API key** tab → Generate.
2. Open `https://trello.com/1/authorize?expiration=never&scope=read,write,account&response_type=token&name=trelloc&key=YOUR_KEY` → Allow → copy the token.
3. `export TRELLO_API_KEY=… TRELLO_TOKEN=…` in your shell when testing.

Make a scratch board (e.g. "trelloc-sandbox") with a few lists, labels, and the Custom Fields Power-Up enabled.

## 2. First prompt (Phase 0 + 1)

```text
Read CLAUDE.md, then docs/ROADMAP.md, docs/ARCHITECTURE.md and docs/TRELLO-API.md.

Implement Phase 0, then stop and show me the workspace layout and smoke-test output.
After I confirm, implement Phase 1 task by task, one Conventional Commit per task.
No real Trello API calls in tests; use wiremock. Before using any endpoint marked
(verify) in docs/TRELLO-API.md, check it against
https://developer.atlassian.com/cloud/trello/rest/ and remove the marker.
At the end of Phase 1, run the smoke test and report what's done, what deviated from
the docs, and what I should test manually.
```

## 3. Next phases

```text
Phase N of docs/ROADMAP.md. Same rules: read the relevant parts of docs/CLI.md and
docs/MCP.md first, wiremock tests for every client method (happy path, 404, 429-then-success, 429 exhausted,
plain-text error), verify (verify) endpoints, commit per task, smoke test green, report.
```

## 4. Owner checks per phase

- P1: `trelloc auth login`, `trelloc tree`, `trelloc board view trelloc-sandbox`.
- P2: create → update → move (same board, then another board) → archive → delete a card; `trelloc card list --mine --overdue`; run `trelloc mcp serve` in Claude Code and ask it for your cards.
- P3: comment, checklist with items, label create/apply, set a dropdown custom field by option name, upload a file.
- P4: `card bulk move --dry-run`, then for real on the sandbox; `trelloc standup`.
- P5: `trelloc mcp install --client claude-code`, restart, tools appear.
- P6: `release.yml` dry run, then the real v0.1.0 / v1.0.0.

## 5. Keep docs honest

When the API behaves differently from these docs, Claude updates the doc in the same commit. The docs are the spec for the next phase.
