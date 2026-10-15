//! Human-input operations shared by the CLI and the MCP server, so both
//! resolve names, parse dates and shape output the same way.

use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    client::{Filter, Query},
    error::Result,
    model::{Board, BoardSnapshot, Member, Workspace},
    resolve::{BoardId, Resolver},
};

// ── hierarchy ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BoardListInput {
    /// Workspace name or id; omit for every board the token can see.
    pub workspace: Option<String>,
    pub include_closed: bool,
    pub starred: bool,
}

pub async fn board_list(r: &Resolver<'_>, input: &BoardListInput) -> Result<Vec<Board>> {
    let filter = if input.include_closed {
        Filter::All
    } else {
        Filter::Open
    };
    let mut boards = match &input.workspace {
        Some(w) => {
            let ws = r.workspace(w).await?;
            r.client().workspace_boards(&ws.id, filter).await?
        }
        None => r.client().my_boards(filter).await?,
    };
    if input.starred {
        boards.retain(|b| b.starred);
    }
    Ok(boards)
}

/// `board view` / `trello_board_get`: the snapshot plus open-card counts
/// per list (one extra call).
pub async fn board_overview(r: &Resolver<'_>, board: &str) -> Result<Value> {
    let b = r.board(board).await?;
    let snap = r.snapshot(&b).await?;
    let ids: Vec<IdList> = r
        .client()
        .get(
            &format!("/boards/{}/cards", b.id),
            &Query::new().push("fields", "idList"),
        )
        .await?;
    Ok(overview_json(&snap, &ids))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdList {
    pub id_list: String,
}

fn overview_json(snap: &BoardSnapshot, cards: &[IdList]) -> Value {
    let mut lists = snap.lists.clone();
    lists.sort_by(|a, b| {
        a.pos
            .partial_cmp(&b.pos)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    json!({
        "id": snap.id,
        "name": snap.name,
        "url": snap.url,
        "short_link": snap.short_link,
        "workspace_id": snap.workspace_id,
        "lists": lists.iter().map(|l| json!({
            "id": l.id,
            "name": l.name,
            "cards": cards.iter().filter(|c| c.id_list == l.id).count(),
        })).collect::<Vec<_>>(),
        "labels": snap.labels.iter().map(|l| json!({
            "id": l.id, "name": l.name, "color": l.color,
        })).collect::<Vec<_>>(),
        "members": snap.members.iter().map(|m| json!({
            "id": m.id, "username": m.username, "full_name": m.full_name,
        })).collect::<Vec<_>>(),
        "custom_fields": snap.custom_fields.iter().map(|f| json!({
            "id": f.id,
            "name": f.name,
            "type": f.field_type,
            "options": f.options.iter().map(|o| o.value.text.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Members of a board, a workspace, or (neither given) the default board.
pub async fn member_list(
    r: &Resolver<'_>,
    board: Option<&str>,
    workspace: Option<&str>,
) -> Result<Vec<Member>> {
    if let Some(w) = workspace {
        let ws = r.workspace(w).await?;
        return r.client().workspace_members(&ws.id).await;
    }
    let b = r.board_or_default(board).await?;
    Ok(r.snapshot(&b).await?.members)
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeBoard {
    pub id: String,
    pub name: String,
    pub lists: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeWorkspace {
    pub id: Option<String>,
    pub name: String,
    pub boards: Vec<TreeBoard>,
}

/// Workspace → Board → List. Lists come from `/1/batch`, 10 boards per call.
pub async fn tree(r: &Resolver<'_>, workspace: Option<&str>) -> Result<Vec<TreeWorkspace>> {
    let dir = r.directory(false).await?;
    let only = match workspace {
        Some(w) => Some(r.workspace(w).await?.id),
        None => None,
    };
    let boards: Vec<&Board> = dir
        .boards
        .iter()
        .filter(|b| only.is_none() || b.workspace_id == only)
        .collect();
    let urls: Vec<String> = boards
        .iter()
        .map(|b| format!("/boards/{}/lists?fields=name&filter=open", b.id))
        .collect();
    let lists = r.client().batch(&urls).await?;
    let mut groups: Vec<TreeWorkspace> = Vec::new();
    let mut add = |ws: Option<&Workspace>, b: &Board, lists: Vec<String>| {
        let id = ws.map(|w| w.id.clone());
        let pos = match groups.iter().position(|g| g.id == id) {
            Some(i) => i,
            None => {
                groups.push(TreeWorkspace {
                    id: id.clone(),
                    name: ws.map_or_else(|| "Personal".into(), |w| w.display_name.clone()),
                    boards: vec![],
                });
                groups.len() - 1
            }
        };
        groups[pos].boards.push(TreeBoard {
            id: b.id.clone(),
            name: b.name.clone(),
            lists,
        });
    };
    for (b, l) in boards.iter().zip(lists) {
        let names = l
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|x| x["name"].as_str().map(str::to_string))
            .collect();
        let ws = dir
            .workspaces
            .iter()
            .find(|w| Some(&w.id) == b.workspace_id.as_ref());
        add(ws, b, names);
    }
    Ok(groups)
}

/// Board as shown in lists: id, name, shortLink, url, workspace.
pub fn board_summary(b: &Board) -> Value {
    json!({
        "id": b.id,
        "name": b.name,
        "short_link": b.short_link,
        "url": b.url,
        "workspace_id": b.workspace_id,
        "closed": b.closed,
        "starred": b.starred,
    })
}

/// Board id + name for outputs.
pub fn board_ref_json(b: &BoardId) -> Value {
    json!({ "id": b.id, "name": b.name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrelloList;

    #[test]
    fn overview_counts_cards_per_list_in_position_order() {
        let snap = BoardSnapshot {
            id: "b".into(),
            name: "B".into(),
            lists: vec![
                TrelloList {
                    id: "l2".into(),
                    name: "Doing".into(),
                    pos: 2.0,
                    ..Default::default()
                },
                TrelloList {
                    id: "l1".into(),
                    name: "To Do".into(),
                    pos: 1.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let cards = vec![
            IdList {
                id_list: "l1".into(),
            },
            IdList {
                id_list: "l1".into(),
            },
            IdList {
                id_list: "l2".into(),
            },
        ];
        let v = overview_json(&snap, &cards);
        assert_eq!(v["lists"][0]["name"], "To Do");
        assert_eq!(v["lists"][0]["cards"], 2);
        assert_eq!(v["lists"][1]["cards"], 1);
    }
}
