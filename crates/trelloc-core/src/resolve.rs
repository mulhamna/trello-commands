//! Names → ids for workspaces, boards, lists, labels, members and custom
//! fields, with a disk cache (TTL 1 h) and refresh-on-miss. Ambiguous names
//! are an error listing the qualified names, never a silent pick.
//!
//! Cache layout under `{cache_dir}/{member_id or profile}/`:
//! `boards.json` (my workspaces + boards) and `snapshots/{board_id}.json`.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    board_ref::{is_id, BoardRef},
    card_ref::CardRef,
    client::{Filter, TrelloClient},
    config::{CachedMember, TrelloConfig},
    error::{Result, TrelloError},
    model::{Board, BoardSnapshot, CustomField, Label, Member, TrelloList, Workspace},
};

pub const CACHE_TTL_SECS: i64 = 3600;

/// Up to 3 candidates with Jaro-Winkler similarity ≥ 0.8 (case-insensitive).
pub fn suggest(input: &str, candidates: &[String]) -> Vec<String> {
    let needle = input.to_lowercase();
    let mut scored: Vec<(f64, &String)> = candidates
        .iter()
        .map(|c| (strsim::jaro_winkler(&needle, &c.to_lowercase()), c))
        .filter(|(s, _)| *s >= 0.8)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<String> = Vec::new();
    for (_, c) in scored {
        if !out.contains(c) {
            out.push(c.clone());
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Serialize, Deserialize)]
struct Cached<T> {
    fetched_at: i64,
    data: T,
}

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(cache_dir: PathBuf, owner: &str) -> Self {
        Self {
            dir: cache_dir.join(owner),
        }
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    fn load<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let text = std::fs::read_to_string(self.dir.join(name)).ok()?;
        let c: Cached<T> = serde_json::from_str(&text).ok()?;
        (now_secs() - c.fetched_at < CACHE_TTL_SECS).then_some(c.data)
    }

    fn save<T: Serialize>(&self, name: &str, data: &T) {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let payload = Cached {
            fetched_at: now_secs(),
            data,
        };
        if let Ok(text) = serde_json::to_string(&payload) {
            // ponytail: cache write failures are non-fatal; next call refetches.
            let _ = std::fs::write(path, text);
        }
    }

    fn remove(&self, name: &str) {
        let _ = std::fs::remove_file(self.dir.join(name));
    }

    pub fn clear(&self) -> Result<()> {
        if self.dir.exists() {
            std::fs::remove_dir_all(&self.dir)?;
        }
        Ok(())
    }
}

/// My workspaces and open boards: what board-name resolution searches.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Directory {
    pub workspaces: Vec<Workspace>,
    pub boards: Vec<Board>,
}

impl Directory {
    pub fn workspace_name(&self, id: Option<&str>) -> String {
        id.and_then(|id| self.workspaces.iter().find(|w| w.id == id))
            .map(|w| w.display_name.clone())
            .unwrap_or_else(|| "Personal".into())
    }

    /// `Workspace/Board`, for ambiguity errors.
    pub fn qualified(&self, b: &Board) -> String {
        format!(
            "{}/{}",
            self.workspace_name(b.workspace_id.as_deref()),
            b.name
        )
    }
}

/// A resolved board: enough to call the API and to show the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoardId {
    pub id: String,
    pub name: String,
}

pub struct Resolver<'a> {
    client: &'a TrelloClient,
    cache: Cache,
    aliases: BTreeMap<String, String>,
    workspace: Option<String>,
    default_board: Option<String>,
    default_list: Option<String>,
    member: Option<CachedMember>,
    refresh: bool,
    /// Loaded data and whether this resolver fetched it from the network
    /// (a miss on fresh data doesn't trigger another refetch).
    directory: Mutex<Option<(Directory, bool)>>,
    snapshots: Mutex<BTreeMap<String, (BoardSnapshot, bool)>>,
}

fn not_found(kind: &'static str, input: &str, names: &[String]) -> TrelloError {
    TrelloError::resolve(kind, input, suggest(input, names))
}

/// Pick the single item matching `pred`, or an ambiguity error.
fn unique<T: Clone>(
    kind: &'static str,
    input: &str,
    items: &[T],
    label: impl Fn(&T) -> String,
    pred: impl Fn(&T) -> bool,
) -> Result<Option<T>> {
    let hits: Vec<&T> = items.iter().filter(|i| pred(i)).collect();
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        many => Err(TrelloError::Resolve {
            kind,
            input: format!("{input}' is ambiguous; matches: '{}", {
                many.iter()
                    .map(|i| label(i))
                    .collect::<Vec<_>>()
                    .join("', '")
            }),
            suggestions: vec![],
        }),
    }
}

impl<'a> Resolver<'a> {
    pub fn new(client: &'a TrelloClient, cfg: &TrelloConfig) -> Self {
        let owner = cfg
            .member
            .as_ref()
            .map(|m| m.id.clone())
            .unwrap_or_else(|| format!("profile-{}", cfg.profile));
        Self {
            client,
            cache: Cache::new(cfg.cache_dir.clone(), &owner),
            aliases: cfg.aliases.clone(),
            workspace: cfg.workspace.clone(),
            default_board: cfg.default_board.clone(),
            default_list: cfg.default_list.clone(),
            member: cfg.member.clone(),
            refresh: false,
            directory: Mutex::new(None),
            snapshots: Mutex::new(BTreeMap::new()),
        }
    }

    /// Bypass the disk cache (`--refresh`).
    pub fn with_refresh(mut self, refresh: bool) -> Self {
        self.refresh = refresh;
        self
    }

    /// Narrow board-name resolution to one workspace (id or name).
    pub fn with_workspace(mut self, workspace: Option<String>) -> Self {
        if workspace.is_some() {
            self.workspace = workspace;
        }
        self
    }

    pub fn client(&self) -> &TrelloClient {
        self.client
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    pub fn default_board(&self) -> Option<&str> {
        self.default_board.as_deref()
    }

    pub fn default_list(&self) -> Option<&str> {
        self.default_list.as_deref()
    }

    /// The cached member behind `me` (never calls the API).
    pub fn me(&self) -> Result<&CachedMember> {
        self.member.as_ref().ok_or_else(|| {
            TrelloError::Auth(
                "the logged-in member is not cached; run `trelloc whoami --refresh`".into(),
            )
        })
    }

    fn alias(&self, input: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(input))
            .map(|(_, v)| v.as_str())
    }

    // ── directory (workspaces + boards) ─────────────────────────────────

    /// Returns the directory and whether it came from the network just now.
    async fn directory_with_origin(&self, refresh: bool) -> Result<(Directory, bool)> {
        let refresh = refresh || self.refresh;
        if !refresh {
            if let Some(d) = self.directory.lock().expect("directory lock").clone() {
                return Ok(d);
            }
            if let Some(d) = self.cache.load::<Directory>("boards.json") {
                *self.directory.lock().expect("directory lock") = Some((d.clone(), false));
                return Ok((d, false));
            }
        }
        let dir = Directory {
            workspaces: self.client.my_workspaces().await?,
            boards: self.client.my_boards(Filter::Open).await?,
        };
        self.cache.save("boards.json", &dir);
        *self.directory.lock().expect("directory lock") = Some((dir.clone(), true));
        Ok((dir, true))
    }

    pub async fn directory(&self, refresh: bool) -> Result<Directory> {
        Ok(self.directory_with_origin(refresh).await?.0)
    }

    /// Run `find` on the directory; on a miss with cached data, refetch once.
    async fn with_directory<T>(
        &self,
        find: impl Fn(&Directory) -> Result<Option<T>>,
    ) -> Result<(Option<T>, Directory)> {
        let (dir, fresh) = self.directory_with_origin(false).await?;
        if let Some(hit) = find(&dir)? {
            return Ok((Some(hit), dir));
        }
        if fresh {
            return Ok((None, dir));
        }
        let (dir, _) = self.directory_with_origin(true).await?;
        Ok((find(&dir)?, dir))
    }

    /// id | short name | display name.
    pub async fn workspace(&self, input: &str) -> Result<Workspace> {
        let input = input.trim();
        let lower = input.to_lowercase();
        let (hit, dir) = self
            .with_directory(|d| {
                unique(
                    "workspace",
                    input,
                    &d.workspaces,
                    |w| format!("{} ({})", w.display_name, w.name),
                    |w| {
                        w.id == input
                            || w.name.to_lowercase() == lower
                            || w.display_name.to_lowercase() == lower
                    },
                )
            })
            .await?;
        hit.ok_or_else(|| {
            let names: Vec<String> = dir
                .workspaces
                .iter()
                .flat_map(|w| [w.display_name.clone(), w.name.clone()])
                .collect();
            not_found("workspace", input, &names)
        })
    }

    /// Workspace id to narrow board names to, if one is configured.
    async fn workspace_scope(&self) -> Result<Option<String>> {
        match &self.workspace {
            None => Ok(None),
            Some(w) if is_id(w) => Ok(Some(w.clone())),
            Some(w) => Ok(Some(self.workspace(w).await?.id)),
        }
    }

    /// id | shortLink | board URL | alias | name. A board-only alias value or
    /// the board part of a `board/list` alias both work.
    pub async fn board(&self, input: &str) -> Result<BoardId> {
        let input = input.trim();
        if input.is_empty() {
            return Err(TrelloError::Invalid("board is empty".into()));
        }
        let target = match self.alias(input) {
            Some(v) => v.split_once('/').map_or(v, |(b, _)| b),
            None => input,
        };
        let cached_name = |id: &str| {
            self.directory
                .lock()
                .expect("directory lock")
                .clone()
                .map(|(d, _)| d)
                .or_else(|| self.cache.load::<Directory>("boards.json"))
                .and_then(|d| d.boards.into_iter().find(|b| b.id == id))
        };
        match BoardRef::parse(target) {
            BoardRef::Id(id) => {
                // Ids never trigger a directory fetch; the cache only adds a name.
                let name = cached_name(&id).map_or_else(|| id.clone(), |b| b.name);
                return Ok(BoardId { id, name });
            }
            BoardRef::ShortLink(link) => {
                let (hit, _) = self
                    .with_directory(|d| Ok(d.boards.iter().find(|b| b.short_link == link).cloned()))
                    .await?;
                if let Some(b) = hit {
                    return Ok(BoardId {
                        id: b.id,
                        name: b.name,
                    });
                }
                match self.client.board(&link).await {
                    Ok(b) => {
                        return Ok(BoardId {
                            id: b.id,
                            name: b.name,
                        })
                    }
                    // An 8-char word may be a board name after all.
                    Err(TrelloError::NotFound(_)) | Err(TrelloError::Api { status: 400, .. }) => {}
                    Err(e) => return Err(e),
                }
            }
            BoardRef::Name(_) => {}
        }
        self.board_by_name(target).await
    }

    async fn board_by_name(&self, name: &str) -> Result<BoardId> {
        let scope = self.workspace_scope().await?;
        let lower = name.to_lowercase();
        let in_scope = |b: &Board| scope.is_none() || b.workspace_id.as_deref() == scope.as_deref();
        let (hit, dir) = self
            .with_directory(|d| {
                unique(
                    "board",
                    name,
                    &d.boards,
                    |b| d.qualified(b),
                    |b| in_scope(b) && b.name.to_lowercase() == lower,
                )
            })
            .await?;
        if let Some(b) = hit {
            return Ok(BoardId {
                id: b.id,
                name: b.name,
            });
        }
        let names: Vec<String> = dir
            .boards
            .iter()
            .filter(|b| in_scope(b))
            .map(|b| b.name.clone())
            .collect();
        Err(not_found("board", name, &names))
    }

    /// `input`, else the profile's `default_board`.
    pub async fn board_or_default(&self, input: Option<&str>) -> Result<BoardId> {
        match input.or(self.default_board.as_deref()) {
            Some(b) => self.board(b).await,
            None => Err(TrelloError::Invalid(
                "no board given; pass --board or set one with `trelloc board use <board>`".into(),
            )),
        }
    }

    // ── board snapshot ───────────────────────────────────────────────────

    async fn snapshot_with_origin(
        &self,
        board_id: &str,
        refresh: bool,
    ) -> Result<(BoardSnapshot, bool)> {
        let refresh = refresh || self.refresh;
        let file = format!("snapshots/{board_id}.json");
        if !refresh {
            if let Some(s) = self.snapshots.lock().expect("snapshot lock").get(board_id) {
                return Ok(s.clone());
            }
            if let Some(s) = self.cache.load::<BoardSnapshot>(&file) {
                self.remember(board_id, &s, false);
                return Ok((s, false));
            }
        }
        let s = self.client.board_snapshot(board_id).await?;
        self.cache.save(&file, &s);
        self.remember(board_id, &s, true);
        Ok((s, true))
    }

    fn remember(&self, board_id: &str, s: &BoardSnapshot, fresh: bool) {
        self.snapshots
            .lock()
            .expect("snapshot lock")
            .insert(board_id.to_string(), (s.clone(), fresh));
    }

    /// Lists, labels, members and custom fields of a board (cached).
    pub async fn snapshot(&self, board: &BoardId) -> Result<BoardSnapshot> {
        Ok(self.snapshot_with_origin(&board.id, false).await?.0)
    }

    /// Drop a board's cached snapshot after changing its lists/labels/fields.
    pub fn invalidate(&self, board_id: &str) {
        self.snapshots
            .lock()
            .expect("snapshot lock")
            .remove(board_id);
        self.cache.remove(&format!("snapshots/{board_id}.json"));
    }

    /// Drop the cached board directory after creating/renaming boards.
    pub fn invalidate_directory(&self) {
        *self.directory.lock().expect("directory lock") = None;
        self.cache.remove("boards.json");
    }

    async fn with_snapshot<T>(
        &self,
        board: &BoardId,
        find: impl Fn(&BoardSnapshot) -> Result<Option<T>>,
    ) -> Result<(Option<T>, BoardSnapshot)> {
        let (snap, fresh) = self.snapshot_with_origin(&board.id, false).await?;
        if let Some(hit) = find(&snap)? {
            return Ok((Some(hit), snap));
        }
        if fresh {
            return Ok((None, snap));
        }
        let (snap, _) = self.snapshot_with_origin(&board.id, true).await?;
        Ok((find(&snap)?, snap))
    }

    // ── things on a board ───────────────────────────────────────────────

    /// id | name | 1-based position among open lists.
    pub async fn list(&self, board: &BoardId, input: &str) -> Result<TrelloList> {
        let input = input.trim();
        let lower = input.to_lowercase();
        let (hit, snap) = self
            .with_snapshot(board, |s| {
                if let Some(l) = s.lists.iter().find(|l| l.id == input) {
                    return Ok(Some(l.clone()));
                }
                if let Some(l) = unique(
                    "list",
                    input,
                    &s.lists,
                    |l| l.name.clone(),
                    |l| l.name.to_lowercase() == lower,
                )? {
                    return Ok(Some(l));
                }
                let mut open: Vec<&TrelloList> = s.lists.iter().filter(|l| !l.closed).collect();
                open.sort_by(|a, b| {
                    a.pos
                        .partial_cmp(&b.pos)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                Ok(input
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|i| open.get(i).map(|l| (*l).clone())))
            })
            .await?;
        if let Some(mut l) = hit {
            if l.id_board.is_empty() {
                l.id_board = board.id.clone();
            }
            return Ok(l);
        }
        let names: Vec<String> = snap.lists.iter().map(|l| l.name.clone()).collect();
        Err(not_found(
            "list",
            &format!("{}/{input}", board.name),
            &names,
        ))
    }

    /// `Board/List`, a list alias, or a list on `board` (else the default
    /// board). Returns the board too, since moves need it.
    pub async fn list_path(
        &self,
        input: &str,
        board: Option<&str>,
    ) -> Result<(BoardId, TrelloList)> {
        let input = input.trim();
        let target = self.alias(input).unwrap_or(input);
        if let Some((b, l)) = target.rsplit_once('/') {
            // `Board/List`, unless the whole thing is a list name with a slash.
            if let Ok(board) = self.board(b).await {
                if let Ok(list) = self.list(&board, l).await {
                    return Ok((board, list));
                }
            }
        }
        let board = self.board_or_default(board).await?;
        let list = self.list(&board, target).await?;
        Ok((board, list))
    }

    /// id | name | color (unnamed labels are matched by color).
    pub async fn label(&self, board: &BoardId, input: &str) -> Result<Label> {
        let input = input.trim();
        let lower = input.to_lowercase();
        let (hit, snap) = self
            .with_snapshot(board, |s| {
                if let Some(l) = s.labels.iter().find(|l| l.id == input) {
                    return Ok(Some(l.clone()));
                }
                if let Some(l) = unique("label", input, &s.labels, Label::display, |l| {
                    l.name.to_lowercase() == lower
                })? {
                    return Ok(Some(l));
                }
                unique(
                    "label",
                    input,
                    &s.labels,
                    |l| {
                        format!(
                            "{} ({})",
                            l.display(),
                            l.color.as_deref().unwrap_or("no color")
                        )
                    },
                    |l| {
                        l.color
                            .as_deref()
                            .is_some_and(|c| c.eq_ignore_ascii_case(input))
                    },
                )
            })
            .await?;
        hit.ok_or_else(|| {
            let names: Vec<String> = snap.labels.iter().map(Label::display).collect();
            not_found("label", input, &names)
        })
    }

    /// `me` | username (with or without `@`) | full name | id.
    pub async fn member(&self, board: &BoardId, input: &str) -> Result<Member> {
        let input = input.trim().trim_start_matches('@');
        if input.eq_ignore_ascii_case("me") {
            let me = self.me()?;
            return Ok(Member {
                id: me.id.clone(),
                username: me.username.clone().unwrap_or_default(),
                full_name: me.full_name.clone().unwrap_or_default(),
                ..Default::default()
            });
        }
        let lower = input.to_lowercase();
        let (hit, snap) = self
            .with_snapshot(board, |s| {
                if let Some(m) = s.members.iter().find(|m| m.id == input) {
                    return Ok(Some(m.clone()));
                }
                if let Some(m) = s
                    .members
                    .iter()
                    .find(|m| m.username.to_lowercase() == lower)
                {
                    return Ok(Some(m.clone()));
                }
                unique(
                    "member",
                    input,
                    &s.members,
                    |m| format!("{} (@{})", m.full_name, m.username),
                    |m| m.full_name.to_lowercase() == lower,
                )
            })
            .await?;
        hit.ok_or_else(|| {
            let names: Vec<String> = snap
                .members
                .iter()
                .flat_map(|m| [m.username.clone(), m.full_name.clone()])
                .collect();
            not_found("member", input, &names)
        })
    }

    /// id | name.
    pub async fn field(&self, board: &BoardId, input: &str) -> Result<CustomField> {
        let input = input.trim();
        let lower = input.to_lowercase();
        let (hit, snap) = self
            .with_snapshot(board, |s| {
                if let Some(f) = s.custom_fields.iter().find(|f| f.id == input) {
                    return Ok(Some(f.clone()));
                }
                unique(
                    "custom field",
                    input,
                    &s.custom_fields,
                    |f| f.name.clone(),
                    |f| f.name.to_lowercase() == lower,
                )
            })
            .await?;
        hit.ok_or_else(|| {
            if snap.custom_fields.is_empty() {
                return TrelloError::Unavailable(format!(
                    "board '{}' has no custom fields (is the Custom Fields Power-Up enabled?)",
                    snap.name
                ));
            }
            let names: Vec<String> = snap.custom_fields.iter().map(|f| f.name.clone()).collect();
            not_found("custom field", input, &names)
        })
    }

    // ── cards ───────────────────────────────────────────────────────────

    /// id | shortLink | card URL | `board#42` | `#42` (default board) → an id
    /// usable in `/cards/{id}`. Only `#n` forms cost a request.
    pub async fn card(&self, input: &str) -> Result<String> {
        match CardRef::parse(input)? {
            CardRef::Id(s) | CardRef::ShortLink(s) => Ok(s),
            CardRef::BoardNumber { board, number } => {
                let b = self.board_or_default(board.as_deref()).await?;
                self.client
                    .card_numbers(&b.id)
                    .await?
                    .into_iter()
                    .find(|(_, n)| *n == number)
                    .map(|(id, _)| id)
                    .ok_or_else(|| {
                        TrelloError::NotFound(format!("card #{number} on board '{}'", b.name))
                    })
            }
        }
    }
}
