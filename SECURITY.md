# Security Policy

trelloc handles a Trello API key and user token, which together grant access to every board their owner can see. Security reports are taken seriously and handled privately.

## Supported versions

| Version | Supported |
| --- | --- |
| Latest release | ✓ |
| Older releases | Fixes ship in a new release; please upgrade |

## Reporting a vulnerability

**Please don't open a public issue.** Report privately through
[GitHub Security Advisories](https://github.com/mulhamna/trello-commands/security/advisories/new).

Include, as far as you can:

- the affected version (`trelloc --version`) and install channel,
- what an attacker can do and what they need (local access, a malicious MCP client, a crafted API response, …),
- steps or a proof of concept — **with real tokens, workspace names and card data removed**.

What to expect:

- an acknowledgement within a few days,
- an assessment and, if confirmed, a fix in a patch release with a GitHub Security Advisory crediting you (unless you'd rather stay anonymous),
- coordinated disclosure: please give us a reasonable window to ship the fix before publishing details.

## Scope

In scope, for example:

- the API key or token being printed, logged, written into MCP client configs, or sent anywhere other than the Trello API,
- destructive MCP tools running without `confirm: true`, or destructive CLI commands running without confirmation / `--yes`,
- the install scripts or the npm installer accepting a binary that doesn't match `checksums.txt`,
- path traversal or arbitrary file reads through `trello_attachment_add` or other inputs beyond what the caller explicitly passed,
- weaknesses in the release pipeline (workflow permissions, publishing, provenance).

Out of scope:

- actions an MCP client or AI agent performs with tools you deliberately granted it and confirmed,
- vulnerabilities in Trello itself — report those to Trello,
- issues requiring an already-compromised machine or user account,
- findings from automated scanners without a demonstrated impact.

## How trelloc protects your credentials

- The key and token live in the trelloc config file (created with permissions `600` on Unix) or in `TRELLO_API_KEY` / `TRELLO_TOKEN`; `trelloc mcp install` never writes them into client configs — entries only carry the profile name.
- They are sent only in the `Authorization` header to the configured Trello API base URL (never in a URL), and are redacted from errors, logs and debug output.
- Release binaries are built in GitHub Actions with build provenance attestations; crates.io and npm packages are published through OIDC trusted publishing, so no long-lived publish tokens exist. See [INSTALL.md](INSTALL.md#verify-a-download) to verify a download.
