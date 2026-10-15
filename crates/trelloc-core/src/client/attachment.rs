use std::path::Path;

use serde_json::json;

use super::{Body, Query, TrelloClient};
use crate::{
    error::{Result, TrelloError},
    model::Attachment,
};

const ATTACHMENT_FIELDS: &str = "id,name,url,bytes,mimeType,date,isUpload";

impl TrelloClient {
    pub async fn attachments(&self, card_id: &str) -> Result<Vec<Attachment>> {
        self.get(
            &format!("/cards/{card_id}/attachments"),
            &Query::new().push("fields", ATTACHMENT_FIELDS),
        )
        .await
    }

    /// Multipart upload of a local file (field `file`).
    pub async fn upload_attachment(
        &self,
        card_id: &str,
        path: &Path,
        name: Option<&str>,
        set_cover: bool,
    ) -> Result<Attachment> {
        if !path.is_file() {
            return Err(TrelloError::Invalid(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let bytes = tokio::fs::read(path).await?;
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".into());
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        let mut fields = vec![
            ("name".to_string(), name.unwrap_or(&file_name).to_string()),
            ("mimeType".to_string(), mime.clone()),
        ];
        if set_cover {
            fields.push(("setCover".into(), "true".into()));
        }
        self.call(
            reqwest::Method::POST,
            &format!("/cards/{card_id}/attachments"),
            &Query::new(),
            Some(Body::File {
                name: file_name,
                bytes,
                mime,
                fields,
            }),
        )
        .await
    }

    pub async fn attach_url(
        &self,
        card_id: &str,
        url: &str,
        name: Option<&str>,
        set_cover: bool,
    ) -> Result<Attachment> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(TrelloError::Invalid(
                "attachment URLs must start with http:// or https://".into(),
            ));
        }
        let mut body = json!({ "url": url, "setCover": set_cover });
        if let Some(n) = name {
            body["name"] = json!(n);
        }
        self.post(&format!("/cards/{card_id}/attachments"), body)
            .await
    }

    pub async fn delete_attachment(&self, card_id: &str, attachment_id: &str) -> Result<()> {
        self.delete(&format!("/cards/{card_id}/attachments/{attachment_id}"))
            .await
            .map(drop)
    }
}
