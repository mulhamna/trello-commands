use serde_json::{json, Value};

use super::{Query, TrelloClient};
use crate::{
    error::Result,
    model::{CheckItem, Checklist},
};

impl TrelloClient {
    pub async fn card_checklists(&self, card_id: &str) -> Result<Vec<Checklist>> {
        self.get(
            &format!("/cards/{card_id}/checklists"),
            &Query::new().push("checkItems", "all"),
        )
        .await
    }

    pub async fn create_checklist(&self, card_id: &str, name: &str) -> Result<Checklist> {
        self.post(
            &format!("/cards/{card_id}/checklists"),
            json!({ "name": name }),
        )
        .await
    }

    pub async fn delete_checklist(&self, checklist_id: &str) -> Result<()> {
        self.delete(&format!("/checklists/{checklist_id}"))
            .await
            .map(drop)
    }

    /// `due` is ISO; `member_id` assigns the item (Trello paid feature).
    pub async fn add_check_item(
        &self,
        checklist_id: &str,
        name: &str,
        due: Option<&str>,
        member_id: Option<&str>,
    ) -> Result<CheckItem> {
        let mut body = json!({ "name": name, "pos": "bottom", "checked": false });
        if let Some(d) = due {
            body["due"] = json!(d);
        }
        if let Some(m) = member_id {
            body["idMember"] = json!(m);
        }
        self.post(&format!("/checklists/{checklist_id}/checkItems"), body)
            .await
    }

    /// `body`: any of `state` (`complete`/`incomplete`), `name`, `due`, `pos`.
    pub async fn update_check_item(
        &self,
        card_id: &str,
        item_id: &str,
        body: Value,
    ) -> Result<CheckItem> {
        self.put(&format!("/cards/{card_id}/checkItem/{item_id}"), body)
            .await
    }

    pub async fn delete_check_item(&self, checklist_id: &str, item_id: &str) -> Result<()> {
        self.delete(&format!("/checklists/{checklist_id}/checkItems/{item_id}"))
            .await
            .map(drop)
    }
}
