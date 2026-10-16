pub mod auth;
pub mod board_access;
pub mod bulk;
pub mod card;
pub mod collab;
pub mod hierarchy;
pub mod mcp;
pub mod report;

use std::io::Read;

use anyhow::{Context, Result};
use trelloc_core::{
    config::{BoardAccess, BoardAccessFile, TrelloConfig},
    Resolver, TrelloClient,
};

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
    board_access: BoardAccess,
}

impl Session {
    pub fn resolver(&self) -> Resolver<'_> {
        Resolver::new(&self.client, &self.cfg)
            .with_refresh(self.refresh)
            .with_workspace(self.workspace.clone())
            .with_board_access(self.board_access.clone())
    }
}

impl Ctx {
    pub fn config(&self) -> Result<TrelloConfig> {
        Ok(TrelloConfig::load_profile(self.profile.as_deref())?)
    }

    pub fn session(&self) -> Result<Session> {
        let cfg = self.config()?;
        let client = TrelloClient::new(&cfg)?;
        let board_access = BoardAccessFile::load()?.access(&cfg.profile);
        Ok(Session {
            cfg,
            client,
            board_access,
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
