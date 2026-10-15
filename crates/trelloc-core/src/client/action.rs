use super::{Query, TrelloClient};
use crate::{error::Result, model::Action};

/// Collection reads cap at 1000 per request.
pub const ACTION_LIMIT: usize = 1000;

impl TrelloClient {
    /// A board's activity; `since`/`before` are ISO dates or action ids.
    pub async fn board_actions(
        &self,
        board_id: &str,
        filter: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Action>> {
        self.get(
            &format!("/boards/{board_id}/actions"),
            &Query::new()
                .push_opt("filter", filter)
                .push_opt("since", since)
                .push("limit", limit.clamp(1, ACTION_LIMIT)),
        )
        .await
    }

    /// The token member's own actions (nested route: not members-limited).
    pub async fn my_actions(
        &self,
        filter: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Action>> {
        self.get(
            "/members/me/actions",
            &Query::new()
                .push_opt("filter", filter)
                .push_opt("since", since)
                .push("limit", limit.clamp(1, ACTION_LIMIT)),
        )
        .await
    }

    pub async fn card_actions(
        &self,
        card_id: &str,
        filter: Option<&str>,
        page: u32,
    ) -> Result<Vec<Action>> {
        self.get(
            &format!("/cards/{card_id}/actions"),
            &Query::new()
                .push_opt("filter", filter)
                .push("page", page.min(super::comment::MAX_ACTION_PAGE)),
        )
        .await
    }
}
