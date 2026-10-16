//! Config file (`~/.config/trelloc/config.toml`) with named profiles and
//! `TRELLO_*` env overrides. Priority: env > selected profile > defaults.

use crate::error::{Result, TrelloError};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Which boards are available to commands using a profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoardAccess {
    /// Legacy profiles remain unrestricted until configured explicitly.
    #[default]
    Unconfigured,
    All,
    AllExcept(BTreeSet<String>),
    Selected(BTreeSet<String>),
}

impl BoardAccess {
    pub fn is_configured(&self) -> bool {
        !matches!(self, Self::Unconfigured)
    }

    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    pub fn is_restricted(&self) -> bool {
        matches!(self, Self::Selected(_) | Self::AllExcept(_))
    }

    pub fn is_active(&self, board_id: &str) -> bool {
        match self {
            Self::Unconfigured | Self::All => true,
            Self::AllExcept(ids) => !ids.contains(board_id),
            Self::Selected(ids) => ids.contains(board_id),
        }
    }

    pub fn activate(&mut self, board_id: impl Into<String>) {
        let id = board_id.into();
        match self {
            Self::Unconfigured | Self::All => {}
            Self::AllExcept(ids) => {
                ids.remove(&id);
            }
            Self::Selected(ids) => {
                ids.insert(id);
            }
        }
    }

    pub fn deactivate(&mut self, board_id: impl Into<String>) {
        let id = board_id.into();
        match self {
            Self::Unconfigured => {
                *self = Self::AllExcept(BTreeSet::from([id]));
            }
            Self::All => {
                *self = Self::AllExcept(BTreeSet::from([id]));
            }
            Self::AllExcept(ids) => {
                ids.insert(id);
            }
            Self::Selected(ids) => {
                ids.remove(&id);
            }
        }
    }

    pub fn select<I, S>(&mut self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        *self = Self::Selected(ids.into_iter().map(Into::into).collect());
    }

    pub fn set_all(&mut self) {
        *self = Self::All;
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Cached at login so commands never need `/1/members/me`
    /// (that route has its own 100 requests / 15 min budget).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// Default workspace (organization id) for board lookups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_board: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_list: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_card_limit: Option<usize>,
    /// Short names → board or `board/list` refs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
}

pub const DEFAULT_BASE_URL: &str = "https://api.trello.com/1";
pub const DEFAULT_PROFILE: &str = "default";
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_CARD_LIMIT: usize = 200;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

/// Board activation policies stored separately from the public profile structs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BoardAccessFile {
    profiles: BTreeMap<String, BoardAccess>,
}

impl BoardAccessFile {
    pub fn path() -> Result<PathBuf> {
        Ok(ConfigFile::path()?.with_file_name("board-access.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        warn_if_world_readable(&path);
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text).map_err(|e| TrelloError::Config(format!("{}: {e}", path.display())))
    }

    pub fn access(&self, profile: &str) -> BoardAccess {
        self.profiles.get(profile).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, profile: impl Into<String>, access: BoardAccess) {
        self.profiles.insert(profile.into(), access);
    }

    pub fn remove(&mut self, profile: &str) -> bool {
        self.profiles.remove(profile).is_some()
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| TrelloError::Config(e.to_string()))?;
        std::fs::write(&path, text)?;
        restrict_permissions(&path)?;
        Ok(path)
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_parse<T: std::str::FromStr>(key: &str) -> Result<Option<T>> {
    match env(key) {
        None => Ok(None),
        Some(v) => v
            .parse()
            .map(Some)
            .map_err(|_| TrelloError::Config(format!("{key} has an invalid value '{v}'"))),
    }
}

impl ConfigFile {
    /// `$TRELLO_CONFIG_DIR/config.toml`, else `<config dir>/trelloc/config.toml`.
    pub fn path() -> Result<PathBuf> {
        let dir = match env("TRELLO_CONFIG_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => dirs::config_dir()
                .ok_or_else(|| TrelloError::Config("cannot locate a config directory".into()))?
                .join("trelloc"),
        };
        Ok(dir.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        warn_if_world_readable(&path);
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text).map_err(|e| TrelloError::Config(format!("{}: {e}", path.display())))
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| TrelloError::Config(e.to_string()))?;
        std::fs::write(&path, text)?;
        restrict_permissions(&path)?;
        Ok(path)
    }

    /// `TRELLO_PROFILE` > `current_profile` > `default`.
    pub fn selected_profile_name(&self) -> String {
        env("TRELLO_PROFILE")
            .or_else(|| self.current_profile.clone())
            .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
    }

    pub fn profile_mut(&mut self, name: &str) -> &mut Profile {
        self.profiles.entry(name.to_string()).or_default()
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
#[cfg(unix)]
fn warn_if_world_readable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.permissions().mode() & 0o077 != 0 {
            tracing::warn!(
                "{} is readable by other users; run `chmod 600` on it",
                path.display()
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_world_readable(_path: &std::path::Path) {}

/// The logged-in member, cached in the profile.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CachedMember {
    pub id: String,
    pub username: Option<String>,
    pub full_name: Option<String>,
}

/// Effective settings after merging the selected profile with env overrides.
#[derive(Debug, Clone)]
pub struct TrelloConfig {
    pub profile: String,
    pub api_key: Option<String>,
    pub token: Option<String>,
    pub member: Option<CachedMember>,
    pub workspace: Option<String>,
    pub default_board: Option<String>,
    pub default_list: Option<String>,
    pub timezone: Option<String>,
    pub timeout_secs: u64,
    pub default_card_limit: usize,
    pub aliases: BTreeMap<String, String>,
    pub base_url: String,
    pub cache_dir: PathBuf,
    pub config_path: PathBuf,
}

impl TrelloConfig {
    pub fn load() -> Result<Self> {
        Self::load_profile(None)
    }

    pub fn load_profile(profile: Option<&str>) -> Result<Self> {
        let file = ConfigFile::load()?;
        Self::from_file(&file, profile)
    }

    pub fn from_file(file: &ConfigFile, profile: Option<&str>) -> Result<Self> {
        let name = profile
            .map(str::to_string)
            .unwrap_or_else(|| file.selected_profile_name());
        let p = file.profiles.get(&name).cloned().unwrap_or_default();
        let cache_dir = match env("TRELLO_CACHE_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("trelloc"),
        };
        // A token from the env belongs to whoever the env says; don't pair
        // it with a member cached for a different token.
        let env_token = env("TRELLO_TOKEN");
        let member = match (&env_token, p.member_id) {
            (None, Some(id)) => Some(CachedMember {
                id,
                username: p.username,
                full_name: p.full_name,
            }),
            _ => None,
        };
        Ok(Self {
            profile: name,
            api_key: env("TRELLO_API_KEY").or(p.api_key),
            token: env_token.or(p.token),
            member,
            workspace: env("TRELLO_WORKSPACE").or(p.workspace),
            default_board: env("TRELLO_DEFAULT_BOARD").or(p.default_board),
            default_list: env("TRELLO_DEFAULT_LIST").or(p.default_list),
            timezone: env("TRELLO_TIMEZONE").or(p.timezone),
            timeout_secs: env_parse("TRELLO_TIMEOUT_SECS")?
                .or(p.timeout_secs)
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
            default_card_limit: env_parse("TRELLO_CARD_LIMIT")?
                .or(p.default_card_limit)
                .unwrap_or(DEFAULT_CARD_LIMIT),
            aliases: p.aliases,
            base_url: env("TRELLO_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            cache_dir,
            config_path: ConfigFile::path()?,
        })
    }

    /// Minimal config for tests and embedding: credentials + base URL, no file.
    pub fn with_credentials(
        api_key: impl Into<String>,
        token: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            profile: DEFAULT_PROFILE.to_string(),
            api_key: Some(api_key.into()),
            token: Some(token.into()),
            member: None,
            workspace: None,
            default_board: None,
            default_list: None,
            timezone: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            default_card_limit: DEFAULT_CARD_LIMIT,
            aliases: BTreeMap::new(),
            base_url: base_url.into(),
            cache_dir: std::env::temp_dir().join("trelloc"),
            config_path: PathBuf::new(),
        }
    }

    pub fn credentials(&self) -> Result<(&str, &str)> {
        let key = self.api_key.as_deref().ok_or_else(|| {
            TrelloError::Auth(
                "no API key configured; run `trelloc auth login` or set TRELLO_API_KEY".into(),
            )
        })?;
        let token = self.token.as_deref().ok_or_else(|| {
            TrelloError::Auth(
                "no token configured; run `trelloc auth login` or set TRELLO_TOKEN".into(),
            )
        })?;
        Ok((key, token))
    }

    pub fn member(&self) -> Result<&CachedMember> {
        self.member.as_ref().ok_or_else(|| {
            TrelloError::Auth(
                "the logged-in member is not cached; run `trelloc whoami --refresh` or `trelloc auth login`"
                    .into(),
            )
        })
    }

    pub fn masked_key(&self) -> Option<String> {
        self.api_key.as_deref().map(mask_secret)
    }

    pub fn masked_token(&self) -> Option<String> {
        self.token.as_deref().map(mask_secret)
    }
}

/// `abcde…yz` style preview that is safe to print.
pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 12 {
        return "***".into();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_access_transitions_and_legacy_default() {
        let mut access = BoardAccess::Unconfigured;
        assert!(!access.is_configured());
        assert!(access.is_active("b1"));
        access.deactivate("b1");
        assert!(access.is_restricted());
        assert!(!access.is_active("b1"));
        assert!(access.is_active("b2"));
        access.activate("b1");
        assert!(access.is_active("b1"));
        access.select(["b2".to_string()]);
        assert!(!access.is_active("b1"));
        assert!(access.is_active("b2"));
        access.activate("b1");
        assert!(access.is_active("b1"));
        access.deactivate("b2");
        assert!(!access.is_active("b2"));
        access.set_all();
        access.deactivate("b1");
        assert!(matches!(&access, BoardAccess::AllExcept(_)));
        assert!(!access.is_active("b1"));
        assert!(access.is_active("b2"));
        access.activate("b1");
        assert!(access.is_active("b1"));
        access.set_all();
        assert!(access.is_all());
        assert!(access.is_active("b1"));
        assert_eq!(
            BoardAccessFile::default().access("legacy"),
            BoardAccess::Unconfigured
        );
    }
    use serial_test::serial;

    const VARS: &[&str] = &[
        "TRELLO_CONFIG_DIR",
        "TRELLO_API_KEY",
        "TRELLO_TOKEN",
        "TRELLO_PROFILE",
        "TRELLO_WORKSPACE",
        "TRELLO_DEFAULT_BOARD",
        "TRELLO_DEFAULT_LIST",
        "TRELLO_TIMEOUT_SECS",
        "TRELLO_CARD_LIMIT",
        "TRELLO_BASE_URL",
        "TRELLO_TIMEZONE",
    ];

    fn isolated() -> tempfile::TempDir {
        for v in VARS {
            std::env::remove_var(v);
        }
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("TRELLO_CONFIG_DIR", dir.path());
        dir
    }

    #[test]
    #[serial]
    fn save_load_round_trip() {
        let _dir = isolated();
        let mut file = ConfigFile {
            current_profile: Some("work".into()),
            ..Default::default()
        };
        let p = file.profile_mut("work");
        p.api_key = Some("key".into());
        p.token = Some("tok".into());
        p.member_id = Some("m1".into());
        p.username = Some("alice".into());
        p.aliases.insert("roadmap".into(), "AbCd1234".into());
        let _path = file.save().unwrap();
        let mut board_access = BoardAccessFile::default();
        board_access.set(
            "work",
            BoardAccess::Selected(["AbCd1234".to_string()].into_iter().collect()),
        );
        let access_path = board_access.save().unwrap();
        assert_eq!(
            BoardAccessFile::load().unwrap().access("work"),
            BoardAccess::Selected(["AbCd1234".to_string()].into_iter().collect())
        );
        assert_eq!(ConfigFile::load().unwrap(), file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(_path).unwrap().permissions().mode();
            let access_mode = std::fs::metadata(access_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            assert_eq!(access_mode & 0o777, 0o600);
        }
    }

    #[test]
    #[serial]
    fn env_overrides_profile() {
        let _dir = isolated();
        let mut file = ConfigFile::default();
        let p = file.profile_mut("default");
        p.api_key = Some("file-key".into());
        p.token = Some("file-token".into());
        p.member_id = Some("m1".into());
        p.default_board = Some("AbCd1234".into());
        file.save().unwrap();

        let cfg = TrelloConfig::load().unwrap();
        assert_eq!(cfg.member.as_ref().unwrap().id, "m1");

        std::env::set_var("TRELLO_TOKEN", "env-token");
        std::env::set_var("TRELLO_TIMEOUT_SECS", "5");
        let cfg = TrelloConfig::load().unwrap();
        assert_eq!(cfg.credentials().unwrap(), ("file-key", "env-token"));
        assert!(
            cfg.member.is_none(),
            "cached member belongs to the file token"
        );
        assert_eq!(cfg.default_board.as_deref(), Some("AbCd1234"));
        assert_eq!(cfg.timeout_secs, 5);
        assert_eq!(cfg.default_card_limit, DEFAULT_CARD_LIMIT);
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);

        std::env::set_var("TRELLO_TIMEOUT_SECS", "soon");
        assert!(matches!(TrelloConfig::load(), Err(TrelloError::Config(_))));
        isolated();
    }

    #[test]
    #[serial]
    fn profile_selection_priority() {
        let _dir = isolated();
        let mut file = ConfigFile {
            current_profile: Some("a".into()),
            ..Default::default()
        };
        file.profile_mut("a").workspace = Some("A".into());
        file.profile_mut("b").workspace = Some("B".into());

        let ws = |p: Option<&str>, f: &ConfigFile| TrelloConfig::from_file(f, p).unwrap().workspace;
        assert_eq!(ws(None, &file), Some("A".into()));
        std::env::set_var("TRELLO_PROFILE", "b");
        assert_eq!(ws(None, &file), Some("B".into()));
        assert_eq!(ws(Some("a"), &file), Some("A".into()));
        isolated();
    }

    #[test]
    #[serial]
    fn missing_credentials_are_auth_errors() {
        let _dir = isolated();
        let cfg = TrelloConfig::load().unwrap();
        assert!(matches!(cfg.credentials(), Err(TrelloError::Auth(_))));
        assert!(matches!(cfg.member(), Err(TrelloError::Auth(_))));
        std::env::set_var("TRELLO_API_KEY", "k");
        let cfg = TrelloConfig::load().unwrap();
        assert!(matches!(cfg.credentials(), Err(TrelloError::Auth(m)) if m.contains("token")));
        isolated();
    }

    #[test]
    fn masks_secrets() {
        assert_eq!(mask_secret("0123456789abcdef"), "0123…ef");
        assert_eq!(mask_secret("short"), "***");
    }
}
