#![no_main]

//! User-typed strings reach these parsers from both the CLI and MCP tool
//! input; none of them may panic.

use libfuzzer_sys::fuzz_target;
use trelloc_core::{
    error::redact, filter::parse_field_expr, ops::label_color, time::created_at,
    time::parse_when, BoardRef, CardRef,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > 1024 {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let _ = CardRef::parse(&text);
    let _ = BoardRef::parse(&text);
    let _ = parse_field_expr(&text);
    let _ = parse_when(&text, Some("Asia/Jakarta"));
    let _ = parse_when(&text, None);
    let _ = label_color(&text);
    let _ = created_at(&text);
    let _ = redact(&text);
});
