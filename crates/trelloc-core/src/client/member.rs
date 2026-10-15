use super::{Query, TrelloClient};
use crate::{
    error::Result,
    model::{Member, Workspace},
};

const MEMBER_FIELDS: &str = "id,username,fullName,initials";

impl TrelloClient {
    /// `GET /1/members/me`. Counts against the 100 / 15 min members limit:
    /// call it at login and cache the result, not per command.
    pub async fn me(&self) -> Result<Member> {
        self.get(
            "/members/me",
            &Query::new().push("fields", "id,username,fullName,initials,email"),
        )
        .await
    }

    pub async fn my_workspaces(&self) -> Result<Vec<Workspace>> {
        self.get(
            "/members/me/organizations",
            &Query::new().push("fields", "id,name,displayName,url"),
        )
        .await
    }

    pub async fn workspace_members(&self, workspace_id: &str) -> Result<Vec<Member>> {
        self.get(
            &format!("/organizations/{workspace_id}/members"),
            &Query::new().push("fields", MEMBER_FIELDS),
        )
        .await
    }

    pub async fn board_members(&self, board_id: &str) -> Result<Vec<Member>> {
        self.get(
            &format!("/boards/{board_id}/members"),
            &Query::new().push("fields", MEMBER_FIELDS),
        )
        .await
    }
}
