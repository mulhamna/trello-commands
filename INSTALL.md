# Installing trelloc

Every channel installs the same single `trelloc` binary, which is both the CLI and the MCP server (`trelloc mcp serve`). Prebuilt binaries exist for:

| OS | x86_64 | aarch64 / arm64 |
| --- | --- | --- |
| Linux (glibc 2.17+) | ✓ | ✓ |
| macOS | ✓ | ✓ (Apple silicon) |
| Windows | ✓ | ✓ |

- [Install script (macOS / Linux)](#install-script-macos--linux)
- [Install script (Windows)](#install-script-windows)
- [Homebrew](#homebrew) · [Scoop](#scoop) · [npm](#npm) · [Cargo](#cargo) · [Manual download](#manual-download)
- [Verify a download](#verify-a-download)
- [After installing](#after-installing) · [Set up an AI client](#set-up-an-ai-client)
- [Upgrade](#upgrade) · [Uninstall](#uninstall) · [Troubleshooting](#troubleshooting)

## Install script (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.sh | bash
```

The script detects your OS and architecture, downloads the matching archive from the latest GitHub Release, verifies it against the release's `checksums.txt`, and installs `trelloc` to `~/.local/bin`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `TRELLOC_VERSION` | latest release | Install a specific version, e.g. `v1.0.0` |
| `TRELLOC_INSTALL_DIR` | `~/.local/bin` | Install directory |

```bash
curl -fsSL https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.sh \
  | TRELLOC_VERSION=v1.0.0 TRELLOC_INSTALL_DIR=/usr/local/bin bash
```

If the install directory isn't on your `PATH`, the script prints the line to add to your shell profile.

## Install script (Windows)

```powershell
irm https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.ps1 | iex
```

Installs `trelloc.exe` to `%LOCALAPPDATA%\Programs\trelloc\bin` after verifying the checksum, and adds that folder to your user `PATH` (restart the terminal afterwards). With options:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.ps1))) `
  -Version v1.0.0 -InstallDir "C:\Tools\trelloc" -SkipPathUpdate
```

| Parameter | Default | Purpose |
| --- | --- | --- |
| `-Version` | latest release | Install a specific version |
| `-InstallDir` | `%LOCALAPPDATA%\Programs\trelloc\bin` | Install directory |
| `-SkipPathUpdate` | off | Don't touch the user `PATH` |

## Homebrew

macOS and Linux:

```bash
brew install mulhamna/tap/trelloc
```

## Scoop

Windows:

```powershell
scoop bucket add mulhamna https://github.com/mulhamna/scoop-bucket
scoop install mulhamna/trelloc
```

## npm

```bash
npm i -g @mulham28/trelloc
```

The package is a thin wrapper: on install it downloads the native binary for your platform from the matching GitHub Release and verifies its SHA-256 against `checksums.txt`. Node.js 18+ is required for the installer only — `trelloc` itself doesn't run on Node.

Use it without a global install, e.g. straight from an MCP client:

```json
{ "mcpServers": { "trelloc": { "command": "npx", "args": ["-y", "@mulham28/trelloc", "mcp", "serve"] } } }
```

## Cargo

Builds from source (Rust stable required):

```bash
cargo install trelloc
```

## Manual download

Grab an archive from the [Releases](https://github.com/mulhamna/trello-commands/releases) page:

| Platform | Archive | Binary inside |
| --- | --- | --- |
| Linux x86_64 | `trelloc-linux-x86_64.tar.gz` | `trelloc-linux-x86_64` |
| Linux aarch64 | `trelloc-linux-aarch64.tar.gz` | `trelloc-linux-aarch64` |
| macOS Intel | `trelloc-macos-x86_64.tar.gz` | `trelloc-macos-x86_64` |
| macOS Apple silicon | `trelloc-macos-aarch64.tar.gz` | `trelloc-macos-aarch64` |
| Windows x86_64 | `trelloc-windows-x86_64.zip` | `trelloc-windows-x86_64.exe` |
| Windows arm64 | `trelloc-windows-aarch64.zip` | `trelloc-windows-aarch64.exe` |

Extract it, rename the binary to `trelloc` (`trelloc.exe` on Windows) and put it somewhere on your `PATH`.

## Verify a download

Every release ships a `checksums.txt` (SHA-256) and [build provenance attestations](https://github.com/mulhamna/trello-commands/attestations) produced in GitHub Actions.

```bash
# checksum
shasum -a 256 -c checksums.txt --ignore-missing      # macOS
sha256sum -c checksums.txt --ignore-missing          # Linux

# provenance (requires the GitHub CLI)
gh attestation verify trelloc-macos-aarch64.tar.gz --repo mulhamna/trello-commands
```

The npm package is published with npm provenance, and the crates are published from the same workflow via crates.io trusted publishing.

## After installing

```bash
trelloc --version
trelloc auth login        # API key (trello.com/power-ups/admin) + token from the authorize page
trelloc tree
```

`auth login` asks for an API key, opens Trello's authorize page for a token, checks both once, and caches your member id in the profile (so later commands never spend the `/members/me` budget). Use `--profile <name>` to keep several accounts.

| | Config (`config.toml`, permissions `600`) | Cache (boards, board snapshots) |
| --- | --- | --- |
| Linux | `~/.config/trelloc/` | `~/.cache/trelloc/` |
| macOS | `~/Library/Application Support/trelloc/` | `~/Library/Caches/trelloc/` |
| Windows | `%APPDATA%\trelloc\` | `%LOCALAPPDATA%\trelloc\` |

`TRELLO_CONFIG_DIR` and `TRELLO_CACHE_DIR` override these locations; `TRELLO_API_KEY`, `TRELLO_TOKEN`, `TRELLO_PROFILE`, `TRELLO_WORKSPACE` and `TRELLO_DEFAULT_BOARD` override what's stored in the profile.

## Set up an AI client

```bash
trelloc mcp install            # checklist, then pick clients (and profiles) with checkboxes
trelloc mcp install --client claude-code --client cursor
trelloc mcp doctor             # binary on PATH, credentials, client config
```

Supported targets: `claude-code`, `claude-code-cli`, `claude-desktop`, `cursor`, `gemini-cli`, `codex`, `vscode`, `copilot-cli`, `opencode`, `windsurf`, `zed`, `openclaw`, `hermes`, `antigravity-cli`, `antigravity-desktop`, `arsy-code`, and `generic-json` (prints a snippet to paste anywhere).

The entry is named `trelloc`, so it coexists with Trello's official server. With several profiles selected, each becomes its own entry (`trelloc-<profile>`). The API key and token are never written into client configs — the server reads them from the trelloc config.

## Upgrade

| Channel | Command |
| --- | --- |
| Install script | re-run the install command |
| Homebrew | `brew upgrade trelloc` |
| Scoop | `scoop update trelloc` |
| npm | `npm i -g @mulham28/trelloc@latest` |
| Cargo | `cargo install trelloc --force` |

MCP client entries point at the `trelloc` binary, so they pick up the new version on the client's next restart.

## Uninstall

| Channel | Command |
| --- | --- |
| Install script (macOS / Linux) | `rm ~/.local/bin/trelloc` (or your `TRELLOC_INSTALL_DIR`) |
| Install script (Windows) | delete `%LOCALAPPDATA%\Programs\trelloc` and remove it from your user `PATH` |
| Homebrew | `brew uninstall trelloc` |
| Scoop | `scoop uninstall trelloc` |
| npm | `npm rm -g @mulham28/trelloc` |
| Cargo | `cargo uninstall trelloc` |

To remove your stored key and token as well, run `trelloc auth logout` per profile or delete the config directory above. Remove the `trelloc` entry from your MCP clients' configs (or re-run `trelloc mcp doctor` to see where they live).

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `trelloc: command not found` | The install directory isn't on `PATH`; add it (the install script prints the exact line) or open a new terminal on Windows. |
| npm: `binary not installed yet` | The postinstall step was skipped (e.g. `--ignore-scripts`). Run `npm rebuild -g @mulham28/trelloc`. |
| `Checksum mismatch` | The download was corrupted or tampered with — retry; if it persists, [open an issue](https://github.com/mulhamna/trello-commands/issues). |
| macOS blocks a manually downloaded binary | Remove the quarantine flag: `xattr -d com.apple.quarantine /path/to/trelloc` (the install script, Homebrew and npm don't need this). |
| MCP client shows no `trello_*` tools | `trelloc mcp doctor`, then restart the client. Check that `trelloc auth status` shows a key and token. |
| `rate_limited` / `Rate limited (API_TOKEN_LIMIT_EXCEEDED)` | Trello allows 100 requests per 10 s per token (300 per key). trelloc paces itself at 90 and retries 429s; another app sharing the token can still push it over. |
| Token prompt shows nothing when pasting | Input is masked (`****`); paste and press Enter. Alternatively: `echo "$TOKEN" \| trelloc auth login --api-key … --token -`. |
