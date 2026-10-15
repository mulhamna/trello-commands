//! HTTP plumbing: auth header, pacing, retry on 429, error-body parsing.
//! Resource methods live in sibling modules as `impl TrelloClient` blocks.

mod board;
mod member;

pub use board::Filter;

use std::sync::Arc;

use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Method, RequestBuilder, StatusCode,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    config::TrelloConfig,
    error::{redact, Result, TrelloError},
    rate_limit::{RateLimiter, RouteClass, MAX_RETRIES},
};

/// `GET /1/batch` takes at most this many URLs.
pub const BATCH_SIZE: usize = 10;

/// Query builder that keeps insertion order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query(Vec<(String, String)>);

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(mut self, key: &str, value: impl ToString) -> Self {
        self.0.push((key.to_string(), value.to_string()));
        self
    }

    pub fn push_opt(self, key: &str, value: Option<impl ToString>) -> Self {
        match value {
            Some(v) => self.push(key, v),
            None => self,
        }
    }

    pub fn pairs(&self) -> &[(String, String)] {
        &self.0
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `a=1&b=2`, for building batch urls.
    pub fn encoded(&self) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&self.0)
            .finish()
    }
}

#[allow(dead_code)] // TEMP: writers arrive with the card client
#[derive(Debug, Clone)]
pub(crate) enum Body {
    Json(Value),
    File {
        name: String,
        bytes: Vec<u8>,
        mime: String,
        /// Extra multipart text fields (`name`, `setCover`, …).
        fields: Vec<(String, String)>,
    },
}

#[derive(Clone)]
pub struct TrelloClient {
    http: reqwest::Client,
    base: String,
    timezone: Option<String>,
    limiter: Arc<RateLimiter>,
}

impl std::fmt::Debug for TrelloClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrelloClient")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

/// `OAuth oauth_consumer_key="{key}", oauth_token="{token}"`, marked
/// sensitive so it never shows up in debug output.
pub fn auth_header(key: &str, token: &str) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(&format!(
        r#"OAuth oauth_consumer_key="{key}", oauth_token="{token}""#
    ))
    .map_err(|_| TrelloError::Auth("API key or token contains invalid characters".into()))?;
    value.set_sensitive(true);
    Ok(value)
}

impl TrelloClient {
    pub fn new(cfg: &TrelloConfig) -> Result<Self> {
        let (key, token) = cfg.credentials()?;
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth_header(key, token)?);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .user_agent(concat!("trelloc/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            base: cfg.base_url.trim_end_matches('/').to_string(),
            timezone: cfg.timezone.clone(),
            limiter: Arc::new(RateLimiter::default()),
        })
    }

    /// Share one limiter across clients (e.g. per token in the MCP server).
    pub fn with_limiter(mut self, limiter: Arc<RateLimiter>) -> Self {
        self.limiter = limiter;
        self
    }

    pub fn limiter(&self) -> &Arc<RateLimiter> {
        &self.limiter
    }

    pub fn timezone(&self) -> Option<&str> {
        self.timezone.as_deref()
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str, q: &Query) -> Result<T> {
        self.call(Method::GET, path, q, None).await
    }

    #[allow(dead_code)] // TEMP: writers arrive with the card client
    /// Write params go in a JSON body, never the URL: long descriptions and
    /// comments would hit URL length limits and end up in logs.
    pub(crate) async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        self.call(Method::POST, path, &Query::new(), Some(Body::Json(body)))
            .await
    }

    #[allow(dead_code)] // TEMP
    pub(crate) async fn put<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        self.call(Method::PUT, path, &Query::new(), Some(Body::Json(body)))
            .await
    }

    #[allow(dead_code)] // TEMP
    pub(crate) async fn delete(&self, path: &str) -> Result<Value> {
        self.call(Method::DELETE, path, &Query::new(), None).await
    }

    pub(crate) async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        q: &Query,
        body: Option<Body>,
    ) -> Result<T> {
        let value = self.send(method, path, q, body.as_ref()).await?;
        Ok(serde_json::from_value(value)?)
    }

    fn build(&self, method: &Method, path: &str, q: &Query, body: Option<&Body>) -> RequestBuilder {
        let mut req = self
            .http
            .request(method.clone(), format!("{}{}", self.base, path));
        if !q.pairs().is_empty() {
            req = req.query(q.pairs());
        }
        match body {
            Some(Body::Json(v)) => req.json(v),
            Some(Body::File {
                name,
                bytes,
                mime,
                fields,
            }) => {
                let part = reqwest::multipart::Part::bytes(bytes.clone()).file_name(name.clone());
                let part = part.mime_str(mime).unwrap_or_else(|_| {
                    reqwest::multipart::Part::bytes(bytes.clone()).file_name(name.clone())
                });
                let form = fields
                    .iter()
                    .fold(reqwest::multipart::Form::new(), |f, (k, v)| {
                        f.text(k.clone(), v.clone())
                    })
                    .part("file", part);
                req.multipart(form)
            }
            None => req,
        }
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        q: &Query,
        body: Option<&Body>,
    ) -> Result<Value> {
        let route = RouteClass::for_path(path);
        let mut attempt = 0;
        loop {
            self.limiter.acquire(route).await;
            let resp = self.build(&method, path, q, body).send().await?;
            let status = resp.status();
            self.limiter.observe(resp.headers());
            tracing::debug!(
                %method,
                path,
                status = status.as_u16(),
                remaining = ?self.limiter.remaining(),
                "trello request"
            );
            let text = resp.text().await?;

            if status == StatusCode::TOO_MANY_REQUESTS {
                if attempt >= MAX_RETRIES {
                    return Err(TrelloError::RateLimit {
                        limit: limit_name(&text),
                    });
                }
                attempt += 1;
                let delay = self.limiter.backoff(attempt);
                tracing::debug!(?delay, attempt, "429 from Trello; backing off");
                tokio::time::sleep(delay).await;
                continue;
            }

            if status.is_success() {
                if text.trim().is_empty() {
                    return Ok(Value::Null);
                }
                return Ok(serde_json::from_str(&text)?);
            }
            return Err(parse_error(status.as_u16(), &text));
        }
    }

    /// Escape hatch for `trelloc api` / `trello_api_request`. `path` is
    /// relative to `/1` and may carry a query string; `query` adds more.
    pub async fn raw(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        body: Option<Value>,
    ) -> Result<Value> {
        let method = Method::from_bytes(method.trim().to_ascii_uppercase().as_bytes())
            .map_err(|_| TrelloError::Invalid(format!("invalid HTTP method '{method}'")))?;
        let mut path = path.trim().to_string();
        if let Some(rest) = path.strip_prefix("/1/") {
            path = format!("/{rest}");
        }
        if !path.starts_with('/') {
            return Err(TrelloError::Invalid(
                "path must start with '/' (e.g. /members/me/boards)".into(),
            ));
        }
        let (path, qs) = match path.split_once('?') {
            Some((p, qs)) => (p.to_string(), qs.to_string()),
            None => (path, String::new()),
        };
        let q = [qs.as_str(), query.unwrap_or_default()]
            .iter()
            .flat_map(|s| url::form_urlencoded::parse(s.trim_start_matches('?').as_bytes()))
            .fold(Query::new(), |q, (k, v)| q.push(&k, v));
        if q.pairs()
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("key") || k.eq_ignore_ascii_case("token"))
        {
            return Err(TrelloError::Invalid(
                "don't pass key/token in the query; credentials are sent as a header".into(),
            ));
        }
        self.send(method, &path, &q, body.map(Body::Json).as_ref())
            .await
    }

    /// `GET /1/batch` in chunks of 10. `urls` are paths relative to `/1`
    /// with their own query strings (e.g. `/boards/abc/cards?fields=name`).
    /// One result per url, in order.
    pub async fn batch(&self, urls: &[String]) -> Result<Vec<Result<Value>>> {
        let mut out = Vec::with_capacity(urls.len());
        for chunk in urls.chunks(BATCH_SIZE) {
            let q = Query::new().push("urls", chunk.join(","));
            let items: Vec<Value> = self.get("/batch", &q).await?;
            if items.len() != chunk.len() {
                return Err(TrelloError::Api {
                    status: 200,
                    code: None,
                    message: format!(
                        "batch returned {} results for {} urls",
                        items.len(),
                        chunk.len()
                    ),
                });
            }
            out.extend(items.into_iter().map(batch_item));
        }
        Ok(out)
    }
}

/// Batch items are `{"200": body}` on success, or an error object keyed by
/// its status (`{"404": "…"}`) or carrying `statusCode`.
fn batch_item(item: Value) -> Result<Value> {
    if let Value::Object(map) = &item {
        if map.len() == 1 {
            let (k, v) = map.iter().next().expect("one entry");
            if let Ok(status) = k.parse::<u16>() {
                if (200..300).contains(&status) {
                    return Ok(v.clone());
                }
                let text = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                return Err(parse_error(status, &text));
            }
        }
        if let Some(status) = map.get("statusCode").and_then(Value::as_u64) {
            return Err(parse_error(status as u16, &item.to_string()));
        }
    }
    Ok(item)
}

/// Pull the limit name (`API_TOKEN_LIMIT_EXCEEDED`, …) out of a 429 body.
fn limit_name(text: &str) -> String {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let candidates = [
        v.get("error").and_then(Value::as_str),
        v.get("message").and_then(Value::as_str),
        Some(text),
    ];
    for c in candidates.into_iter().flatten() {
        if let Some(word) = c
            .split(|ch: char| !(ch.is_ascii_uppercase() || ch == '_'))
            .find(|w| w.ends_with("_EXCEEDED"))
        {
            return word.to_string();
        }
    }
    let t = text.trim();
    if t.is_empty() {
        "rate limit".into()
    } else {
        redact(&t.chars().take(100).collect::<String>())
    }
}

/// JSON `{message|error}` or plain text (Trello uses both).
pub(crate) fn parse_error(status: u16, text: &str) -> TrelloError {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let str_of = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let code = str_of("error").filter(|e| e.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
    let message = str_of("message")
        .or_else(|| str_of("error"))
        .or_else(|| match &v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            let t = text.trim();
            if t.is_empty() {
                StatusCode::from_u16(status)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("request failed")
                    .to_string()
            } else {
                t.chars().take(500).collect()
            }
        });
    let message = redact(&message);
    match status {
        401 => TrelloError::Auth(message),
        404 => TrelloError::NotFound(message),
        _ => TrelloError::Api {
            status,
            code,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_and_plain_errors() {
        assert!(
            matches!(parse_error(401, "invalid token"), TrelloError::Auth(m) if m == "invalid token")
        );
        assert!(matches!(
            parse_error(404, "The requested resource was not found."),
            TrelloError::NotFound(_)
        ));
        match parse_error(400, "invalid value for idList") {
            TrelloError::Api {
                status,
                message,
                code,
            } => {
                assert_eq!(status, 400);
                assert_eq!(message, "invalid value for idList");
                assert!(code.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
        match parse_error(
            429,
            r#"{"message":"Too many","error":"API_TOKEN_DB_LIMIT_EXCEEDED"}"#,
        ) {
            TrelloError::Api { code, message, .. } => {
                assert_eq!(code.as_deref(), Some("API_TOKEN_DB_LIMIT_EXCEEDED"));
                assert_eq!(message, "Too many");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            matches!(parse_error(500, ""), TrelloError::Api { message, .. } if message == "Internal Server Error")
        );
    }

    #[test]
    fn limit_names() {
        assert_eq!(
            limit_name(r#"{"error":"API_TOKEN_LIMIT_EXCEEDED","message":"Rate limit exceeded"}"#),
            "API_TOKEN_LIMIT_EXCEEDED"
        );
        assert_eq!(
            limit_name("API_KEY_LIMIT_EXCEEDED"),
            "API_KEY_LIMIT_EXCEEDED"
        );
        assert_eq!(limit_name(""), "rate limit");
    }

    #[test]
    fn batch_items() {
        assert_eq!(
            batch_item(serde_json::json!({"200": {"id": "a"}})).unwrap(),
            serde_json::json!({"id": "a"})
        );
        assert!(matches!(
            batch_item(serde_json::json!({"404": "not found"})),
            Err(TrelloError::NotFound(_))
        ));
        assert!(matches!(
            batch_item(serde_json::json!({"name":"x","message":"bad","statusCode":400})),
            Err(TrelloError::Api { status: 400, .. })
        ));
    }

    #[test]
    fn auth_header_shape() {
        let h = auth_header("k1", "t1").unwrap();
        assert!(h.is_sensitive());
        assert_eq!(
            h.to_str().unwrap(),
            r#"OAuth oauth_consumer_key="k1", oauth_token="t1""#
        );
        assert!(format!("{h:?}").contains("Sensitive"));
    }

    #[test]
    fn query_encodes() {
        let q = Query::new().push("a", "x y").push_opt("b", None::<String>);
        assert_eq!(q.encoded(), "a=x+y");
        assert_eq!(q.get("a"), Some("x y"));
    }
}
