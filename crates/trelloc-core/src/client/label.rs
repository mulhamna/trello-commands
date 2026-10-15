use serde_json::{json, Value};

use super::TrelloClient;
use crate::{error::Result, model::Label};

impl TrelloClient {
    /// `color` may be `None` for a colorless label.
    pub async fn create_label(
        &self,
        board_id: &str,
        name: &str,
        color: Option<&str>,
    ) -> Result<Label> {
        self.post(
            "/labels",
            json!({ "name": name, "color": color, "idBoard": board_id }),
        )
        .await
    }

    /// `body`: `name` and/or `color`.
    pub async fn update_label(&self, label_id: &str, body: Value) -> Result<Label> {
        self.put(&format!("/labels/{label_id}"), body).await
    }

    pub async fn delete_label(&self, label_id: &str) -> Result<()> {
        self.delete(&format!("/labels/{label_id}")).await.map(drop)
    }
}
