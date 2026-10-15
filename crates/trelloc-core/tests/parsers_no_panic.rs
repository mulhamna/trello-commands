//! Cheap stand-in for the fuzz target: feed deterministic pseudo-random
//! (often invalid UTF-8) inputs to every user-input parser and make sure none
//! of them panic. The real fuzzing runs in CI via ClusterFuzzLite.

use trelloc_core::{
    error::redact, filter::parse_field_expr, ops::label_color, time::created_at, time::parse_when,
    BoardRef, CardRef,
};

/// xorshift64 — deterministic, no rand dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

const ALPHABET: &[u8] =
    b"+-=<>!~/:#_ 0123456789abcdefdhmswAbCdtT.\x00\xc3\xa9\xe2\x82\xac\xf0\x9f\x98\x80\xf6\xd9\xdf";

#[test]
fn parsers_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..50_000 {
        let len = (rng.next() % 32) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| ALPHABET[(rng.next() % ALPHABET.len() as u64) as usize])
            .collect();
        let text = String::from_utf8_lossy(&bytes);
        let _ = CardRef::parse(&text);
        let _ = BoardRef::parse(&text);
        let _ = parse_field_expr(&text);
        let _ = parse_when(&text, Some("Asia/Jakarta"));
        let _ = label_color(&text);
        let _ = created_at(&text);
        let _ = redact(&text);
    }
}

#[test]
fn url_shaped_inputs() {
    for s in [
        "https://trello.com/c/",
        "https://trello.com/c/é",
        "https://trello.com/b/AbCd1234/é",
        "https://trello.com",
        "http://",
        "trello.com/c/AbCd1234",
        "#",
        "##1",
        "a#1#2",
    ] {
        let _ = CardRef::parse(s);
        let _ = BoardRef::parse(s);
    }
}
