//! Parse what a user types for a card: id, shortLink, card URL,
//! `board#42` or `#42` (on the default board).

use crate::{
    board_ref::{is_id, is_short_link, short_link_from_url},
    error::{Result, TrelloError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardRef {
    /// 24-hex object id.
    Id(String),
    /// 8-char shortLink; usable anywhere an id is.
    ShortLink(String),
    /// Per-board card number (`idShort`). `board` is a board id, shortLink,
    /// alias or name; `None` means the default board.
    BoardNumber { board: Option<String>, number: u32 },
}

impl CardRef {
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim();
        if let Some(link) = short_link_from_url(s, "c") {
            return Ok(Self::ShortLink(link));
        }
        if is_id(s) {
            return Ok(Self::Id(s.to_ascii_lowercase()));
        }
        if let Some((board, number)) = s.rsplit_once('#') {
            let number: u32 = number
                .trim()
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid(input))?;
            let board = board.trim();
            return Ok(Self::BoardNumber {
                board: (!board.is_empty()).then(|| board.to_string()),
                number,
            });
        }
        if is_short_link(s) {
            return Ok(Self::ShortLink(s.to_string()));
        }
        Err(invalid(input))
    }

    /// The id to put in `/cards/{id}`, when no lookup is needed.
    pub fn direct_id(&self) -> Option<&str> {
        match self {
            Self::Id(s) | Self::ShortLink(s) => Some(s),
            Self::BoardNumber { .. } => None,
        }
    }
}

fn invalid(input: &str) -> TrelloError {
    TrelloError::Invalid(format!(
        "'{input}' is not a card reference (use an id, shortLink, card URL, board#42 or #42)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> CardRef {
        CardRef::parse(s).unwrap()
    }

    #[test]
    fn parses_card_inputs() {
        assert_eq!(
            p("5F2B0C0E1A2B3C4D5E6F7A8B"),
            CardRef::Id("5f2b0c0e1a2b3c4d5e6f7a8b".into())
        );
        assert_eq!(p("AbCd1234"), CardRef::ShortLink("AbCd1234".into()));
        assert_eq!(
            p("https://trello.com/c/AbCd1234/42-fix-login"),
            CardRef::ShortLink("AbCd1234".into())
        );
        assert_eq!(
            p("http://www.trello.com/c/AbCd1234"),
            CardRef::ShortLink("AbCd1234".into())
        );
        assert_eq!(
            p("roadmap#42"),
            CardRef::BoardNumber {
                board: Some("roadmap".into()),
                number: 42
            }
        );
        assert_eq!(
            p("Q4 Plan # 7"),
            CardRef::BoardNumber {
                board: Some("Q4 Plan".into()),
                number: 7
            }
        );
        assert_eq!(
            p("#42"),
            CardRef::BoardNumber {
                board: None,
                number: 42
            }
        );
        assert_eq!(p("#42").direct_id(), None);
        assert_eq!(p("AbCd1234").direct_id(), Some("AbCd1234"));
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "#",
            "#0",
            "#-1",
            "board#x",
            "board#99999999999",
            "hello world",
            "AbCd12345",
            "https://trello.com/b/AbCd1234/board",
            "é#1é",
        ] {
            assert!(CardRef::parse(bad).is_err(), "{bad:?}");
        }
    }
}
