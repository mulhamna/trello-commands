use serde_json::json;

use super::{Query, TrelloClient};
use crate::{error::Result, model::Action};

/// Card actions come 50 per page; Trello allows pages 0..=19.
pub const COMMENT_PAGE_SIZE: usize = 50;
pub const MAX_ACTION_PAGE: u32 = 19;

impl TrelloClient {
    /// One page (newest first) of a card's comments.
    pub async fn comments(&self, card_id: &str, page: u32) -> Result<Vec<Action>> {
        self.get(
            &format!("/cards/{card_id}/actions"),
            &Query::new()
                .push("filter", "commentCard")
                .push("page", page.min(MAX_ACTION_PAGE)),
        )
        .await
    }

    /// Markdown; `@username` mentions notify.
    pub async fn add_comment(&self, card_id: &str, text: &str) -> Result<Action> {
        self.post(
            &format!("/cards/{card_id}/actions/comments"),
            json!({ "text": text }),
        )
        .await
    }

    pub async fn edit_comment(&self, card_id: &str, action_id: &str, text: &str) -> Result<Action> {
        self.put(
            &format!("/cards/{card_id}/actions/{action_id}/comments"),
            json!({ "text": text }),
        )
        .await
    }

    /// Only comment actions can be deleted.
    pub async fn delete_comment(&self, action_id: &str) -> Result<()> {
        self.delete(&format!("/actions/{action_id}"))
            .await
            .map(drop)
    }
}
