## Summary

<!-- What does this PR do and why? Link issues with "Fixes #123". -->

## Changes

<!-- Bullet list of the actual edits per area (core / cli / mcp / ci / docs). -->

## Type of change

- [ ] Bug fix (`fix:`)
- [ ] New feature (`feat:`)
- [ ] Refactor / cleanup (`refactor:`)
- [ ] Docs / chore / CI
- [ ] Breaking change (`feat!:` / `BREAKING CHANGE:`)

## Test plan

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all`
- [ ] New or changed behavior in `crates/` has tests (wiremock for HTTP — never the real Trello API)
- [ ] New Trello endpoints were checked against the official reference (no leftover **(verify)** in `docs/TRELLO-API.md`)

## Security checklist

- [ ] No API keys, tokens, credentials or personal workspace data in code, tests, fixtures or logs
- [ ] No URLs built outside `trelloc-core` (CLI and MCP go through the client)
- [ ] Destructive CLI commands confirm unless `--yes`; destructive MCP tools require `confirm: true`
- [ ] No new `unsafe` without explanation
- [ ] New dependencies are from crates.io with permissive licenses (`cargo deny check` passes)
- [ ] Changes to `.github/workflows/` or release/packaging files are explained below

## Merge behavior

- [ ] Safe to auto-merge once checks pass (add the `automerge` label)
- [ ] Needs manual merge (release timing, rollout, or other risk)

## Notes for reviewer

<!-- Anything that needs extra attention? -->
