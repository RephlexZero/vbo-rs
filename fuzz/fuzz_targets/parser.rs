#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use racelogic_vbo::{ParseOptions, Parser};

fuzz_target!(|data: &[u8]| {
    let parser = Parser::new(ParseOptions {
        strict: false,
        max_line_bytes: 64 * 1024,
        max_rows: 10_000,
        max_issues: 1_000,
    });
    let _ = parser.parse_reader_recovering(Cursor::new(data));
});
