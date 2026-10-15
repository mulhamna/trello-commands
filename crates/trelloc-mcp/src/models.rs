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

// ── collaboration ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Card id, shortLink, card URL, or "board#42".
    pub card: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommentListArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// How many of the latest comments (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommentAddArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Markdown; "@username" mentions notify that member.
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommentEditArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Comment (action) id from trello_comment_list.
    pub comment_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommentDeleteArgs {
    #[serde(flatten)]
    pub account: Account,
    pub comment_id: String,
    /// Must be true.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChecklistAddArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    pub name: String,
    pub items: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChecklistItemAddArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Checklist name or id on the card.
    pub checklist: String,
    pub text: String,
    pub due: Option<String>,
    /// Member to assign: "me", username or id.
    pub member: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChecklistItemUpdateArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Item id, exact text, or a unique part of the text.
    pub item: String,
    pub checked: Option<bool>,
    /// New text.
    pub text: Option<String>,
    /// Human date, or "none" to clear.
    pub due: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChecklistItemDeleteArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    pub item: String,
    /// Must be true.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChecklistDeleteArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Checklist name or id.
    pub checklist: String,
    /// Must be true.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LabelCreateArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Board (default: the profile's default board).
    pub board: Option<String>,
    pub name: String,
    /// green, yellow, orange, red, purple, blue, sky, lime, pink, black (optionally _dark/_light), or "none".
    pub color: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LabelUpdateArgs {
    #[serde(flatten)]
    pub account: Account,
    pub board: Option<String>,
    /// Label name, color or id.
    pub label: String,
    pub name: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LabelDeleteArgs {
    #[serde(flatten)]
    pub account: Account,
    pub board: Option<String>,
    pub label: String,
    /// Must be true: removes the label from every card.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FieldSetArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Custom field name or id.
    pub field: String,
    /// Dropdown option name, number, date ("fri", ISO), true/false, or text.
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FieldClearArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    pub field: String,
    /// Must be true.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AttachmentAddArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    /// Local file to upload (absolute path on the machine running the server).
    pub path: Option<String>,
    /// Or an http(s) URL to attach.
    pub url: Option<String>,
    pub name: Option<String>,
    /// Use as the card cover.
    pub set_cover: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AttachmentDeleteArgs {
    #[serde(flatten)]
    pub account: Account,
    pub card: String,
    pub attachment_id: String,
    /// Must be true.
    pub confirm: Option<bool>,
}

// ── bulk, management, reports ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BulkCreateArgs {
    #[serde(flatten)]
    pub account: Account,
    /// List for every card (name on the default board, id, or "Board/List"); a card's own `list` wins.
    pub list: Option<String>,
    pub cards: Vec<CardCreateItem>,
    /// Return the plan without creating anything.
    pub dry_run: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CardCreateItem {
    pub name: String,
    pub list: Option<String>,
    pub desc: Option<String>,
    pub members: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
    pub due: Option<String>,
    pub custom_fields: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BulkChangesArg {
    /// Move to this list ("List" or "Board/List").
    pub list: Option<String>,
    pub labels_add: Option<Vec<String>>,
    pub labels_remove: Option<Vec<String>>,
    pub members_add: Option<Vec<String>>,
    /// Date or "none".
    pub due: Option<String>,
    pub due_complete: Option<bool>,
    /// true archives every card.
    pub archive: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BulkUpdateArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Explicit card refs. Give either `cards` or `filter`.
    pub cards: Option<Vec<String>>,
    /// Same input as trello_card_search.
    pub filter: Option<CardSearchArgs>,
    pub changes: BulkChangesArg,
    /// Return matching cards and the changes without applying them.
    pub dry_run: Option<bool>,
    /// Required (true) when more than one card would change.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BatchArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Steps run in order: {"op":"create", "name", "list", …}, {"op":"update", "card", …},
    /// {"op":"move", "card", "list"}, {"op":"comment", "card", "text"},
    /// {"op":"archive", "card"}, {"op":"delete", "card"}.
    #[schemars(schema_with = "any_json_array")]
    pub ops: Vec<Value>,
    pub dry_run: Option<bool>,
    /// Required (true) for more than one op or any delete.
    pub confirm: Option<bool>,
}

fn any_json_array(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "type": "array", "items": { "type": "object", "additionalProperties": true } })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BoardCreateArgs {
    #[serde(flatten)]
    pub account: Account,
    pub name: String,
    /// Copy lists, labels and cards from this board.
    pub from_board: Option<String>,
    /// Add To Do / Doing / Done (default true; ignored with from_board).
    pub default_lists: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BoardUpdateArgs {
    #[serde(flatten)]
    pub account: Account,
    pub board: String,
    pub name: Option<String>,
    pub desc: Option<String>,
    /// true archives the board, false restores it.
    pub closed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BoardDeleteArgs {
    #[serde(flatten)]
    pub account: Account,
    pub board: String,
    /// Must be true: deletes every list and card on the board.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListCreateArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Board (default: the profile's default board).
    pub board: Option<String>,
    pub name: String,
    /// "top", "bottom" or a number.
    pub pos: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListUpdateArgs {
    #[serde(flatten)]
    pub account: Account,
    pub board: Option<String>,
    /// List name or id (archived lists: id).
    pub list: String,
    pub name: Option<String>,
    /// "top", "bottom" or a number.
    pub pos: Option<String>,
    /// true archives the list, false restores it.
    pub closed: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct StandupArgs {
    #[serde(flatten)]
    pub account: Account,
    /// Default "yesterday".
    pub since: Option<String>,
    pub board: Option<String>,
    /// Username; default: me.
    pub member: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ActivityArgs {
    #[serde(flatten)]
    pub account: Account,
    pub board: Option<String>,
    pub card: Option<String>,
    pub since: Option<String>,
    pub member: Option<String>,
    /// Default 50.
    pub limit: Option<usize>,
}
