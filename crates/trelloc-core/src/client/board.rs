use super::{Query, TrelloClient};
use crate::{
    error::Result,
    model::{Board, BoardSnapshot, CustomField, Label, TrelloList},
};

pub(crate) const BOARD_FIELDS: &str = "id,name,desc,shortLink,url,idOrganization,closed,starred";

/// `open`, `closed` or `all`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Open,
    Closed,
    All,
}

impl Filter {
    pub fn as_str(self) -> &'static str {
        match self {
            Filter::Open => "open",
            Filter::Closed => "closed",
            Filter::All => "all",
        }
    }
}

impl TrelloClient {
    /// Boards the token's member can see (nested route: not members-limited).
    pub async fn my_boards(&self, filter: Filter) -> Result<Vec<Board>> {
        self.get(
            "/members/me/boards",
            &Query::new()
                .push("filter", filter.as_str())
                .push("fields", BOARD_FIELDS),
        )
        .await
    }

    pub async fn workspace_boards(&self, workspace_id: &str, filter: Filter) -> Result<Vec<Board>> {
        self.get(
            &format!("/organizations/{workspace_id}/boards"),
            &Query::new()
                .push("filter", filter.as_str())
                .push("fields", BOARD_FIELDS),
        )
        .await
    }

    /// `id` may be a board id or shortLink.
    pub async fn board(&self, id: &str) -> Result<Board> {
        self.get(
            &format!("/boards/{id}"),
            &Query::new().push("fields", BOARD_FIELDS),
        )
        .await
    }

    /// Lists, labels, members and custom fields in one call.
    pub async fn board_snapshot(&self, id: &str) -> Result<BoardSnapshot> {
        self.get(&format!("/boards/{id}"), &snapshot_query()).await
    }

    pub async fn board_lists(&self, board_id: &str, filter: Filter) -> Result<Vec<TrelloList>> {
        self.get(
            &format!("/boards/{board_id}/lists"),
            &Query::new()
                .push("filter", filter.as_str())
                .push("fields", "id,name,closed,pos,idBoard"),
        )
        .await
    }

    pub async fn board_labels(&self, board_id: &str) -> Result<Vec<Label>> {
        self.get(
            &format!("/boards/{board_id}/labels"),
            &Query::new()
                .push("fields", "id,idBoard,name,color")
                .push("limit", 1000),
        )
        .await
    }

    pub async fn board_fields(&self, board_id: &str) -> Result<Vec<CustomField>> {
        self.get(&format!("/boards/{board_id}/customFields"), &Query::new())
            .await
    }
}

pub(crate) fn snapshot_query() -> Query {
    Query::new()
        .push("fields", "name,url,shortLink,idOrganization")
        .push("lists", "open")
        .push("list_fields", "name,pos,closed,idBoard")
        .push("labels", "all")
        .push("label_fields", "name,color,idBoard")
        .push("labels_limit", 1000)
        .push("members", "all")
        .push("member_fields", "fullName,username,initials")
        .push("customFields", "true")
}
