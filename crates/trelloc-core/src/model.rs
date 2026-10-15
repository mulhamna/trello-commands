//! Trello objects. Tolerant serde: unknown fields are ignored and missing
//! ones default, so `fields=` restrictions never break deserialization.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub username: String,
    pub full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initials: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// A Trello workspace. The API calls it `organization`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    /// Short name used in URLs.
    pub name: String,
    pub display_name: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Board {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub short_link: String,
    pub url: String,
    #[serde(rename = "idOrganization")]
    pub workspace_id: Option<String>,
    pub closed: bool,
    pub starred: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct TrelloList {
    pub id: String,
    pub name: String,
    pub closed: bool,
    pub pos: f64,
    pub id_board: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub id_board: String,
    pub name: String,
    pub color: Option<String>,
}

impl Label {
    /// Name, or the color for unnamed labels.
    pub fn display(&self) -> String {
        match (self.name.is_empty(), &self.color) {
            (false, _) => self.name.clone(),
            (true, Some(c)) => c.clone(),
            (true, None) => self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct FieldOption {
    pub id: String,
    pub value: OptionValue,
    pub color: Option<String>,
    pub pos: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OptionValue {
    pub text: String,
}

/// Custom field definition (board-scoped, needs the Custom Fields Power-Up).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomField {
    pub id: String,
    pub id_model: String,
    pub name: String,
    /// `text`, `number`, `date`, `checkbox` or `list` (dropdown).
    #[serde(rename = "type")]
    pub field_type: String,
    pub options: Vec<FieldOption>,
    pub pos: f64,
}

/// One `GET /1/boards/{id}?lists=open&labels=all&members=all&customFields=true`:
/// everything name resolution needs for a board. The resolver's cache unit.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct BoardSnapshot {
    pub id: String,
    pub name: String,
    pub url: String,
    pub short_link: String,
    #[serde(rename = "idOrganization")]
    pub workspace_id: Option<String>,
    pub lists: Vec<TrelloList>,
    pub labels: Vec<Label>,
    pub members: Vec<Member>,
    pub custom_fields: Vec<CustomField>,
}

impl BoardSnapshot {
    pub fn list_name(&self, id: &str) -> Option<&str> {
        self.lists
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.name.as_str())
    }

    pub fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Badges {
    pub comments: u32,
    pub attachments: u32,
    pub check_items: u32,
    pub check_items_checked: u32,
    pub description: bool,
}

/// A custom field value on a card. `value` is `{"text"|"number"|"date"|
/// "checked": "…"}`; dropdowns use `id_value` (the option id).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomFieldItem {
    pub id: String,
    pub id_custom_field: String,
    pub id_value: Option<String>,
    pub value: Option<serde_json::Value>,
}

impl CustomFieldItem {
    /// Human value: option text, the number/text/date, or `true`/`false`.
    pub fn display_value(&self, field: Option<&CustomField>) -> String {
        if let Some(opt) = &self.id_value {
            return field
                .and_then(|f| f.options.iter().find(|o| &o.id == opt))
                .map(|o| o.value.text.clone())
                .unwrap_or_else(|| opt.clone());
        }
        let Some(v) = &self.value else {
            return String::new();
        };
        for key in ["text", "number", "date", "checked"] {
            if let Some(x) = v.get(key) {
                return match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
            }
        }
        v.to_string()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub name: String,
    /// `complete` or `incomplete`.
    pub state: String,
    pub pos: f64,
    pub due: Option<String>,
    pub id_member: Option<String>,
    pub id_checklist: String,
}

impl CheckItem {
    pub fn done(&self) -> bool {
        self.state == "complete"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Checklist {
    pub id: String,
    pub name: String,
    pub id_card: String,
    pub pos: f64,
    pub check_items: Vec<CheckItem>,
}

impl Checklist {
    /// `(done, total)`.
    pub fn progress(&self) -> (usize, usize) {
        (
            self.check_items.iter().filter(|i| i.done()).count(),
            self.check_items.len(),
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub url: String,
    pub bytes: Option<u64>,
    pub mime_type: Option<String>,
    pub date: Option<String>,
    pub is_upload: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    /// Per-board card number (`#42`).
    pub id_short: u32,
    pub short_link: String,
    pub short_url: String,
    pub url: String,
    pub name: String,
    pub desc: String,
    pub closed: bool,
    pub id_board: String,
    pub id_list: String,
    pub id_members: Vec<String>,
    pub id_labels: Vec<String>,
    pub labels: Vec<Label>,
    pub pos: f64,
    pub due: Option<String>,
    pub start: Option<String>,
    pub due_complete: bool,
    pub date_last_activity: Option<String>,
    pub badges: Badges,
    pub cover: Option<serde_json::Value>,
    pub subscribed: bool,
    pub custom_field_items: Vec<CustomFieldItem>,
    pub checklists: Vec<Checklist>,
    pub attachments: Vec<Attachment>,
}

impl Card {
    /// Created time from the id (no extra call).
    pub fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        crate::time::created_at(&self.id)
    }

    pub fn has_cover(&self) -> bool {
        self.cover.as_ref().is_some_and(|c| {
            ["color", "idAttachment", "idUploadedBackground", "url"]
                .iter()
                .any(|k| c.get(*k).is_some_and(|v| !v.is_null()))
        })
    }
}

/// An action from the activity feed (comments are `commentCard` actions).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    #[serde(rename = "type")]
    pub action_type: String,
    pub date: String,
    pub id_member_creator: String,
    pub member_creator: Option<Member>,
    pub data: serde_json::Value,
}

impl Action {
    pub fn text(&self) -> Option<&str> {
        self.data.get("text").and_then(serde_json::Value::as_str)
    }

    pub fn card_id(&self) -> Option<&str> {
        self.data
            .get("card")
            .and_then(|c| c.get("id"))
            .and_then(serde_json::Value::as_str)
    }

    pub fn card_name(&self) -> Option<&str> {
        self.data
            .get("card")
            .and_then(|c| c.get("name"))
            .and_then(serde_json::Value::as_str)
    }

    pub fn author(&self) -> String {
        self.member_creator
            .as_ref()
            .map(|m| m.username.clone())
            .unwrap_or_else(|| self.id_member_creator.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_from_board_response() {
        let s: BoardSnapshot = serde_json::from_value(serde_json::json!({
            "id": "b1", "name": "Roadmap", "shortLink": "AbCd1234", "idOrganization": null,
            "lists": [{"id": "l1", "name": "To Do", "pos": 16384, "closed": false}],
            "labels": [{"id": "lb1", "name": "", "color": "red"}],
            "members": [{"id": "m1", "username": "alice", "fullName": "Alice"}],
            "customFields": [{"id": "f1", "name": "Severity", "type": "list",
                "options": [{"id": "o1", "value": {"text": "High"}, "color": "red", "pos": 1}]}],
            "prefs": {"background": "blue"}
        }))
        .unwrap();
        assert_eq!(s.short_link, "AbCd1234");
        assert_eq!(s.list_name("l1"), Some("To Do"));
        assert_eq!(s.labels[0].display(), "red");
        assert_eq!(s.custom_fields[0].options[0].value.text, "High");
        assert_eq!(s.member("m1").unwrap().full_name, "Alice");
    }

    #[test]
    fn card_values_and_cover() {
        let c: Card = serde_json::from_value(serde_json::json!({
            "id": "5f2b0c0e1a2b3c4d5e6f7a8b", "idShort": 42, "name": "Fix", "due": null,
            "cover": {"color": null, "idAttachment": null},
            "customFieldItems": [
                {"idCustomField": "f1", "idValue": "o1"},
                {"idCustomField": "f2", "value": {"number": "3"}},
                {"idCustomField": "f3", "value": {"checked": "true"}}
            ],
            "badges": {"comments": 2, "checkItems": 3, "checkItemsChecked": 1}
        }))
        .unwrap();
        assert!(!c.has_cover());
        assert!(c.created_at().is_some());
        let f1 = CustomField {
            id: "f1".into(),
            options: vec![FieldOption {
                id: "o1".into(),
                value: OptionValue {
                    text: "High".into(),
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(c.custom_field_items[0].display_value(Some(&f1)), "High");
        assert_eq!(c.custom_field_items[1].display_value(None), "3");
        assert_eq!(c.custom_field_items[2].display_value(None), "true");
        assert_eq!(c.badges.check_items, 3);
    }
}
