#[derive(Debug, thiserror::Error)]
pub enum TrelloError {
    #[error("HTTP error: {}", redact(&.0.to_string()))]
    Http(#[from] reqwest::Error),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("API error {status}{}: {message}", code.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default())]
    Api {
        status: u16,
        code: Option<String>,
        message: String,
    },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Rate limited ({limit})")]
    RateLimit { limit: String },

    #[error("Feature unavailable: {0}")]
    Unavailable(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Could not resolve {kind} '{input}'{}", suggestion_hint(.suggestions))]
    Resolve {
        kind: &'static str,
        input: String,
        suggestions: Vec<String>,
    },

    #[error("Invalid input: {0}")]
    Invalid(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TrelloError>;

fn suggestion_hint(suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        String::new()
    } else {
        format!(" (did you mean: {}?)", suggestions.join(", "))
    }
}

impl TrelloError {
    pub fn resolve(kind: &'static str, input: impl Into<String>, suggestions: Vec<String>) -> Self {
        Self::Resolve {
            kind,
            input: input.into(),
            suggestions,
        }
    }
}

/// Hide anything that looks like a Trello credential: `key=`/`token=` query
/// values, `oauth_*="…"` header parts, `ATTA…` tokens, and hex runs of 32+
/// chars (API keys and legacy tokens). 24-hex object ids are left alone.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 20)].iter().collect();
        let prefix = ["key=", "token=", "oauth_consumer_key=\"", "oauth_token=\""]
            .into_iter()
            .find(|p| {
                rest.to_ascii_lowercase().starts_with(p)
                    && (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
            });
        if let Some(p) = prefix {
            out.push_str(&rest[..p.len()]);
            out.push_str("***");
            i += p.chars().count();
            while i < chars.len() && is_secret_char(chars[i]) {
                i += 1;
            }
            continue;
        }
        if is_secret_char(chars[i]) && (i == 0 || !is_secret_char(chars[i - 1])) {
            let end = (i..chars.len())
                .find(|&j| !is_secret_char(chars[j]))
                .unwrap_or(chars.len());
            let word: String = chars[i..end].iter().collect();
            let hex = word.chars().all(|c| c.is_ascii_hexdigit());
            if (word.starts_with("ATTA") && word.len() >= 32) || (hex && word.len() >= 32) {
                out.push_str("***");
            } else {
                out.push_str(&word);
            }
            i = end;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn is_secret_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials() {
        let key = "0123456789abcdef0123456789abcdef";
        let token = format!("ATTA{}", "f".repeat(64));
        assert_eq!(redact(&format!("k {key} end")), "k *** end");
        assert_eq!(redact(&format!("t={token},x")), "t=***,x");
        assert_eq!(
            redact(&format!("/1/cards?key={key}&token=abc")),
            "/1/cards?key=***&token=***"
        );
        assert_eq!(
            redact(&format!(
                r#"OAuth oauth_consumer_key="{key}", oauth_token="{token}""#
            )),
            r#"OAuth oauth_consumer_key="***", oauth_token="***""#
        );
        // Object ids (24 hex) and normal words stay.
        assert_eq!(
            redact("card 5f2b0c0e1a2b3c4d5e6f7a8b not found"),
            "card 5f2b0c0e1a2b3c4d5e6f7a8b not found"
        );
        assert_eq!(redact("monkey=1 é"), "monkey=1 é");
    }

    #[test]
    fn resolve_message_has_suggestions() {
        let e = TrelloError::resolve("board", "Roadmp", vec!["Roadmap".into()]);
        assert_eq!(
            e.to_string(),
            "Could not resolve board 'Roadmp' (did you mean: Roadmap?)"
        );
    }
}
