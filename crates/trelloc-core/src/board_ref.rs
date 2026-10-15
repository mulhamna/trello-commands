//! Parse what a user types for a board: id, shortLink, board URL, or a name.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardRef {
    /// 24-hex object id.
    Id(String),
    /// 8-char shortLink (works anywhere an id does). An 8-char word could also
    /// be a board name; the resolver tries it as a shortLink first.
    ShortLink(String),
    /// Anything else: an alias or a name for the resolver.
    Name(String),
}

pub fn is_id(s: &str) -> bool {
    s.len() == 24 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_short_link(s: &str) -> bool {
    s.len() == 8 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `https://trello.com/{kind}/{shortLink}/…` → shortLink, for `kind` = `b` or `c`.
pub(crate) fn short_link_from_url(input: &str, kind: &str) -> Option<String> {
    let rest = input
        .trim()
        .strip_prefix("https://")
        .or_else(|| input.trim().strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("trello.com/")?;
    let mut segs = rest.split(['/', '?', '#']);
    if segs.next()? != kind {
        return None;
    }
    let link = segs.next()?;
    is_short_link(link).then(|| link.to_string())
}

impl BoardRef {
    pub fn parse(input: &str) -> Self {
        let s = input.trim();
        if let Some(link) = short_link_from_url(s, "b") {
            return Self::ShortLink(link);
        }
        if is_id(s) {
            return Self::Id(s.to_ascii_lowercase());
        }
        if is_short_link(s) {
            return Self::ShortLink(s.to_string());
        }
        Self::Name(s.to_string())
    }
}

impl fmt::Display for BoardRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(s) | Self::ShortLink(s) | Self::Name(s) => f.write_str(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_board_inputs() {
        assert_eq!(
            BoardRef::parse("5F2B0C0E1A2B3C4D5E6F7A8B"),
            BoardRef::Id("5f2b0c0e1a2b3c4d5e6f7a8b".into())
        );
        assert_eq!(
            BoardRef::parse("AbCd1234"),
            BoardRef::ShortLink("AbCd1234".into())
        );
        assert_eq!(
            BoardRef::parse("https://trello.com/b/AbCd1234/my-board"),
            BoardRef::ShortLink("AbCd1234".into())
        );
        assert_eq!(
            BoardRef::parse("https://trello.com/b/AbCd1234"),
            BoardRef::ShortLink("AbCd1234".into())
        );
        assert_eq!(
            BoardRef::parse(" Roadmap "),
            BoardRef::Name("Roadmap".into())
        );
        // A card URL is not a board.
        assert_eq!(
            BoardRef::parse("https://trello.com/c/AbCd1234/1-x"),
            BoardRef::Name("https://trello.com/c/AbCd1234/1-x".into())
        );
        assert_eq!(BoardRef::parse("Q4 2026"), BoardRef::Name("Q4 2026".into()));
        assert_eq!(BoardRef::parse(""), BoardRef::Name(String::new()));
    }
}
