#![cfg(feature = "fast-float")]

use racelogic_vbo::{FloatParser, ParseError, ParseOptions, Parser};

fn parse_field(field: &str, float_parser: FloatParser) -> Result<f64, ParseError> {
    let input = format!("[column names]\nvalue\n[data]\n{field}\n");
    let vbo = Parser::new(ParseOptions {
        float_parser,
        ..ParseOptions::default()
    })
    .parse_str(&input)?;
    Ok(vbo.value(0, 0).expect("one valid row and one column"))
}

#[test]
fn fast_float_matches_standard_for_representative_valid_fields() {
    for field in ["0", "-0", "42", "-3.125", ".5", "1.", "6.022e23", "-1E-9"] {
        let standard = parse_field(field, FloatParser::Standard).unwrap();
        let fast = parse_field(field, FloatParser::Fast).unwrap();
        assert_eq!(fast.to_bits(), standard.to_bits(), "field: {field}");
    }
}

#[test]
fn fast_float_matches_standard_rejection_for_invalid_fields() {
    for field in ["not-a-number", "1.2.3", "--1", "1e", "0x1p0"] {
        let standard = parse_field(field, FloatParser::Standard).unwrap_err();
        let fast = parse_field(field, FloatParser::Fast).unwrap_err();
        let ParseError::InvalidNumber {
            line: standard_line,
            column: standard_column,
            value: standard_value,
        } = standard
        else {
            panic!("standard backend unexpectedly accepted or changed error for {field}");
        };
        let ParseError::InvalidNumber {
            line: fast_line,
            column: fast_column,
            value: fast_value,
        } = fast
        else {
            panic!("fast backend unexpectedly accepted or changed error for {field}");
        };
        assert_eq!(
            (fast_line, fast_column, fast_value),
            (standard_line, standard_column, standard_value),
            "field: {field}"
        );
    }
}
