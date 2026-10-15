//! Tool inputs. Doc comments become the JSON schema descriptions the model
//! sees, so they are written for the model.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn any_json_object(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "type": "object", "additionalProperties": true })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResponse {
    #[schemars(schema_with = "any_json_object")]
    pub result: Value,
}

/// Which Trello account a call runs against, and which workspace narrows
/// board names. Both optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct Account {
    /// trelloc profile (account) for this call; see trello_profile_list. Default: the server's profile.
    pub profile: Option<String>,
    /// Workspace name or id to narrow board-name lookups. Default: every board the token can see.
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BoardListArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Include archived (closed) boards.
    pub include_closed: Option<bool>,
    /// Only starred boards.
    pub starred: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BoardArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Board id, shortLink, board URL, alias or name. Default: the profile's default board.
    pub board: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct MemberListArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Members of this board (id, shortLink, URL, alias or name).
    pub board: Option<String>,
    /// Members of this workspace instead (name or id).
    pub in_workspace: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApiRequestArgs {
    #[serde(flatten)]
    pub account: Account,
    /// GET, POST, PUT or DELETE.
    pub method: String,
    /// Path relative to /1, e.g. "/boards/{id}/lists".
    pub path: String,
    /// Query parameters (string, number or boolean values). Never key/token.
    pub query: Option<BTreeMap<String, Value>>,
    /// JSON body for POST/PUT.
    pub body: Option<Value>,
    /// Required (true) for anything other than GET.
    pub confirm: Option<bool>,
}

// ── cards ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FieldFilterArg {
    /// Custom field name or id.
    pub field: String,
    /// One of =, !=, <, <=, >, >=, ~ (contains), empty, set.
    pub op: String,
    /// Value to compare (option name, number, date, text, true/false). Omit for empty/set.
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CardSearchArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Boards to scan (id, shortLink, URL, alias or name). Scoped searches are exact; prefer them.
    pub boards: Option<Vec<String>>,
    /// Lists: names on the given/default board, ids, or "Board/List".
    pub lists: Option<Vec<String>>,
    /// Members on the card: "me", usernames, full names or ids (all must match).
    pub members: Option<Vec<String>>,
    /// Label names or colors (all must match).
    pub labels: Option<Vec<String>>,
    /// Due before (e.g. "fri", "2026-10-20", "+3d").
    pub due_before: Option<String>,
    pub due_after: Option<String>,
    /// Due in the past and not marked complete.
    pub overdue: Option<bool>,
    /// No due date.
    pub no_due: Option<bool>,
    /// Due date marked complete (true) or not (false).
    pub due_complete: Option<bool>,
    pub created_after: Option<String>,
    /// Last activity after this date.
    pub active_after: Option<String>,
    /// Any of: attachments, description, members, cover.
    pub has: Option<Vec<String>>,
    /// Custom field filters; need a board in `boards` or `lists`.
    pub fields: Option<Vec<FieldFilterArg>>,
    /// Name or description contains.
    pub text: Option<String>,
    /// Raw Trello search syntax (e.g. "@me label:bug due:week"). Forces /1/search, which may lag a few seconds behind recent writes.
    pub query: Option<String>,
    pub include_archived: Option<bool>,
    /// due | created | activity | pos.
    pub sort: Option<String>,
    /// Default 50, max 1000.
    pub limit: Option<usize>,
    /// "summary" (default) or "full".
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardGetArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Card id, shortLink, card URL, or "board#42".
    pub card: String,
    /// "full" (default here) or "summary".
    pub detail: Option<String>,
    /// Add the latest comments.
    pub include_comments: Option<bool>,
    /// Add the latest activity.
    pub include_activity: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChecklistArg {
    pub name: String,
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardCreateArgs {
    #[serde(flatten)]
    pub account: Account,
    /// List name (on `board` or the default board), id, or "Board/List". Default: the profile's default_list.
    pub list: Option<String>,
    /// Board for a plain list name.
    pub board: Option<String>,
    pub name: String,
    /// Markdown description.
    pub desc: Option<String>,
    /// "top", "bottom" or a number.
    pub pos: Option<String>,
    /// Members: "me", usernames, full names or ids.
    pub members: Option<Vec<String>>,
    /// Label names or colors.
    pub labels: Option<Vec<String>>,
    /// Human date or ISO.
    pub due: Option<String>,
    pub start: Option<String>,
    /// Custom field name → value (dropdown option name, number, date, true/false, text).
    pub custom_fields: Option<BTreeMap<String, Value>>,
    pub checklists: Option<Vec<ChecklistArg>>,
    /// Copy this card (any card ref) instead of starting empty.
    pub copy_from: Option<String>,
    /// With copy_from: "all" (default) or e.g. "checklists,labels,attachments".
    pub keep: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardUpdateArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Card id, shortLink, card URL, or "board#42".
    pub card: String,
    pub name: Option<String>,
    /// Markdown description (replaces it).
    pub desc: Option<String>,
    /// Human date, ISO, or null/"none" to clear.
    pub due: Option<Value>,
    pub start: Option<Value>,
    pub due_complete: Option<bool>,
    pub members_add: Option<Vec<String>>,
    pub members_remove: Option<Vec<String>>,
    pub labels_add: Option<Vec<String>>,
    pub labels_remove: Option<Vec<String>>,
    /// Custom field name → value; null clears that field.
    pub custom_fields: Option<BTreeMap<String, Value>>,
    /// Watch (true) or unwatch (false) the card.
    pub subscribed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardMoveArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Target list name on the card's board, or "Board/List" for another board.
    pub list: String,
    /// "top", "bottom" or a number.
    pub pos: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardCopyArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Target list name (default board) or "Board/List".
    pub list: String,
    /// Name for the copy; default: the original's.
    pub name: Option<String>,
    /// "all" (default) or e.g. "checklists,labels,attachments".
    pub keep: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardArchiveArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Restore an archived card instead.
    pub unarchive: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardConfirmArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Must be true: deletion is permanent.
    pub confirm: Option<bool>,
}
