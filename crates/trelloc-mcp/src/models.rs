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
