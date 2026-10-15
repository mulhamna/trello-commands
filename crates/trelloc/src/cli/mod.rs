pub mod auth;
pub mod hierarchy;
pub mod mcp;
pub mod report;

use std::io::Read;

use anyhow::{Context, Result};
use trelloc_core::{config::TrelloConfig, Resolver, TrelloClient};

/// Global flags shared by every command.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub profile: Option<String>,
    /// `--workspace`: narrows board-name resolution for this command.
    pub workspace: Option<String>,
    pub json: bool,
    pub yes: bool,
    pub refresh: bool,
}

/// Loaded config + client; build a resolver with [`Session::resolver`].
pub struct Session {
    pub cfg: TrelloConfig,
    pub client: TrelloClient,
    refresh: bool,
    workspace: Option<String>,
}

impl Session {
    pub fn resolver(&self) -> Resolver<'_> {
        Resolver::new(&self.client, &self.cfg)
            .with_refresh(self.refresh)
            .with_workspace(self.workspace.clone())
    }
}

impl Ctx {
    pub fn config(&self) -> Result<TrelloConfig> {
        Ok(TrelloConfig::load_profile(self.profile.as_deref())?)
    }

    pub fn session(&self) -> Result<Session> {
        let cfg = self.config()?;
        let client = TrelloClient::new(&cfg)?;
        Ok(Session {
            cfg,
            client,
            refresh: self.refresh,
            workspace: self.workspace.clone(),
        })
    }
}

/// `-` reads stdin, otherwise the value itself.
pub fn text_or_stdin(value: &str) -> Result<String> {
    if value == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(value.to_string())
    }
}

/// `-` reads stdin, otherwise a file path.
pub fn file_or_stdin(path: &str) -> Result<String> {
    if path == "-" {
        text_or_stdin("-")
    } else {
        std::fs::read_to_string(path).with_context(|| format!("reading {path}"))
    }
}
