//! Long help texts with the trelloc banner.

macro_rules! banner {
    () => {
        concat!(
            "  _            _ _            \n",
            " | |_ _ __ ___| | | ___   ___ \n",
            " | __| '__/ _ \\ | |/ _ \\ / __|\n",
            " | |_| | |  __/ | | (_) | (__ \n",
            "  \\__|_|  \\___|_|_|\\___/ \\___|\n",
            "\n",
        )
    };
}

pub const ROOT_LONG_ABOUT: &str = concat!(
    banner!(),
    "Unofficial Trello CLI and MCP server in one binary.\n\n",
    "Quick start:\n",
    "  trelloc auth login                          Store your API key and token\n",
    "  trelloc tree                                Workspaces → Boards → Lists\n",
    "  trelloc board use Roadmap                   Set the default board\n",
    "  trelloc card list --mine                    Your open cards\n",
    "  trelloc card create \"Fix login\" --list Doing --due fri\n",
    "  trelloc standup                             Daily standup summary\n",
    "  trelloc mcp install                         Register the MCP server (interactive picker)\n",
    "  trelloc mcp install --client claude-code    Register with Claude Code directly\n",
    "  trelloc mcp doctor                          Check MCP prerequisites\n\n",
    "Install options:\n",
    "  Script:    curl -fsSL https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.sh | bash\n",
    "  npm:       npm i -g @mulham28/trelloc\n",
    "  Homebrew:  brew install mulhamna/tap/trelloc\n",
    "  Scoop:     scoop bucket add mulhamna https://github.com/mulhamna/scoop-bucket\n",
    "             scoop install mulhamna/trelloc\n",
    "  Cargo:     cargo install trelloc\n\n",
    "Env vars:    TRELLO_API_KEY, TRELLO_TOKEN, TRELLO_PROFILE, TRELLO_DEFAULT_BOARD\n",
    "Version:     use --version or -V (note: -v enables verbose logging)\n",
    "Not affiliated with or endorsed by Atlassian or Trello."
);

pub const AUTH_LONG_ABOUT: &str = concat!(
    banner!(),
    "Manage Trello auth profiles.\n\n",
    "Typical flow:\n",
    "  trelloc auth login                   Paste an API key, authorize, paste the token\n",
    "  trelloc auth status                  Show profile, member and credentials (masked)\n",
    "  trelloc auth profile list            List saved profiles\n",
    "  trelloc auth profile use <name>      Switch the active profile\n",
    "  trelloc auth logout                  Remove the stored key and token"
);

pub const AUTH_LOGIN_LONG_ABOUT: &str = concat!(
    banner!(),
    "Store a Trello API key and user token.\n\n",
    "1. API key: https://trello.com/power-ups/admin → New → API key → Generate.\n",
    "2. Token: trelloc opens the authorize page for your key; click Allow and paste the token.\n",
    "3. trelloc checks the token once and caches your member id in the profile.\n\n",
    "Saved to the trelloc config file (chmod 600 on Unix).\n",
    "Override at runtime with TRELLO_API_KEY / TRELLO_TOKEN / TRELLO_PROFILE.\n\n",
    "Examples:\n",
    "  trelloc auth login\n",
    "  trelloc auth login --profile work --api-key … --token -"
);

pub const MCP_LONG_ABOUT: &str = concat!(
    banner!(),
    "Run trelloc as an MCP server and register it with AI clients.\n\n",
    "  trelloc mcp install                         Interactive: checks prerequisites, then a client picker\n",
    "  trelloc mcp install --client cursor         Non-interactive\n",
    "  trelloc mcp doctor                          Binary, credentials and client config checks\n",
    "  trelloc mcp serve                           stdio transport (what clients launch)\n",
    "  trelloc mcp serve --transport streamable-http --port 8787"
);

/// Plain banner for interactive flows.
#[allow(dead_code)]
pub const BANNER: &str = banner!();
