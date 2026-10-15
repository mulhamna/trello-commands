//! Tool errors with stable string codes (`data.code`) so clients can branch
//! on them. 4xx-style problems → `INVALID_PARAMS`, server-side → `INTERNAL_ERROR`.

use rmcp::{model::ErrorCode, ErrorData};
use serde_json::{json, Map, Value};
use trelloc_core::TrelloError;

#[derive(Debug, Clone)]
pub struct AppError {
    rpc_code: ErrorCode,
    stable_code: &'static str,
    detail: String,
    data: Option<Value>,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    fn new(
        rpc_code: ErrorCode,
        stable_code: &'static str,
        detail: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            rpc_code,
            stable_code,
            detail: detail.into(),
            data,
        }
    }

    pub fn validation(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::INVALID_PARAMS, "validation_error", detail, None)
    }

    pub fn unsafe_operation(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::INVALID_PARAMS, "unsafe_operation", detail, None)
    }

    pub fn auth_missing(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::INVALID_REQUEST, "auth_missing", detail, None)
    }

    pub fn config_error(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::INVALID_REQUEST, "config_error", detail, None)
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::INTERNAL_ERROR, "internal_error", detail, None)
    }

    pub fn code(&self) -> &'static str {
        self.stable_code
    }

    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    pub fn to_mcp(self) -> ErrorData {
        let mut payload = match self.data {
            Some(Value::Object(map)) => map,
            Some(other) => {
                let mut m = Map::new();
                m.insert("context".into(), other);
                m
            }
            None => Map::new(),
        };
        payload.insert("code".into(), json!(self.stable_code));
        ErrorData::new(
            self.rpc_code,
            format!("{}: {}", self.stable_code, self.detail),
            Some(Value::Object(payload)),
        )
    }
}

fn rpc_for_status(status: u16) -> ErrorCode {
    if status >= 500 {
        ErrorCode::INTERNAL_ERROR
    } else {
        ErrorCode::INVALID_PARAMS
    }
}

impl From<TrelloError> for AppError {
    fn from(err: TrelloError) -> Self {
        let detail = trelloc_core::error::redact(&err.to_string());
        match err {
            TrelloError::Auth(_) => Self::auth_missing(detail),
            TrelloError::Config(_) => Self::config_error(detail),
            TrelloError::Invalid(_) => Self::validation(detail),
            TrelloError::Resolve { suggestions, .. } => Self::new(
                ErrorCode::INVALID_PARAMS,
                "resolve_error",
                detail,
                Some(json!({ "suggestions": suggestions })),
            ),
            TrelloError::NotFound(_) => Self::new(
                ErrorCode::INVALID_PARAMS,
                "trello_api_error",
                detail,
                Some(json!({ "status": 404 })),
            ),
            TrelloError::Api { status, code, .. } => Self::new(
                rpc_for_status(status),
                "trello_api_error",
                detail,
                Some(json!({ "status": status, "code": code })),
            ),
            TrelloError::RateLimit { limit } => Self::new(
                ErrorCode::INTERNAL_ERROR,
                "rate_limited",
                detail,
                Some(json!({ "limit": limit })),
            ),
            TrelloError::Unavailable(_) => Self::new(
                ErrorCode::INVALID_PARAMS,
                "unavailable",
                detail,
                Some(json!({
                    "hint": "Enable the Custom Fields Power-Up on the board (paid Trello plans), or check the board's plan limits."
                })),
            ),
            TrelloError::Http(_) | TrelloError::Serialization(_) | TrelloError::Io(_) => {
                Self::internal(detail)
            }
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_errors_to_stable_codes() {
        let e: AppError = TrelloError::resolve("board", "x", vec!["y".into()]).into();
        assert_eq!(e.code(), "resolve_error");
        assert_eq!(e.data().unwrap()["suggestions"], json!(["y"]));

        let e: AppError = TrelloError::Api {
            status: 400,
            code: Some("API_TOKEN_DB_LIMIT_EXCEEDED".into()),
            message: "bad".into(),
        }
        .into();
        assert_eq!(e.code(), "trello_api_error");
        assert_eq!(
            e.data().unwrap()["code"],
            json!("API_TOKEN_DB_LIMIT_EXCEEDED")
        );
        assert_eq!(e.clone().to_mcp().code, ErrorCode::INVALID_PARAMS);

        let e: AppError = TrelloError::Api {
            status: 503,
            code: None,
            message: "down".into(),
        }
        .into();
        assert_eq!(e.to_mcp().code, ErrorCode::INTERNAL_ERROR);

        let e: AppError = TrelloError::RateLimit {
            limit: "API_KEY_LIMIT_EXCEEDED".into(),
        }
        .into();
        assert_eq!(e.code(), "rate_limited");
        assert_eq!(
            e.to_mcp().data.unwrap()["limit"],
            json!("API_KEY_LIMIT_EXCEEDED")
        );

        let e: AppError = TrelloError::Unavailable("no fields".into()).into();
        assert_eq!(e.code(), "unavailable");
        assert!(e.data().unwrap()["hint"]
            .as_str()
            .unwrap()
            .contains("Power-Up"));

        let e: AppError = TrelloError::Auth("no token".into()).into();
        assert_eq!(e.code(), "auth_missing");
    }
}
