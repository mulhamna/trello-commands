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
}
