use std::{
    fmt::Write as _,
    io::{BufReader, Cursor},
};

use racelogic_vbo::{ParseError, ParseIssueKind, ParseOptions, Parser};

const SOURCE: &str = "File created: 2026-07-22\n[header]\n[channel units]\nhhmmss kmh\n[column names]\ntime velocity\n[data]\n120000.0 10.5\n120001.0 20.5\n";

#[test]
fn streaming_visits_rows_without_constructing_a_vbo_table() {
    let mut samples = Vec::new();
    let report = Parser::default()
        .parse_bufread(BufReader::new(Cursor::new(SOURCE)), |sample| {
            assert_eq!(sample.channels()[1].unit.as_deref(), Some("kmh"));
            assert_eq!(
                sample.header().created.as_deref(),
                Some("File created: 2026-07-22")
            );
            samples.push((
                sample.row_index(),
                sample.line_number(),
                sample.values().to_vec(),
            ));
        })
        .expect("stream parses");

    assert_eq!(report.row_count(), 2);
    assert_eq!(report.column_count(), 2);
    assert!(report.issues.is_empty());
    assert_eq!(
        samples,
        vec![(0, 8, vec![120_000.0, 10.5]), (1, 9, vec![120_001.0, 20.5])]
    );
}

#[test]
fn streaming_recovery_skips_invalid_and_oversized_rows() {
    let source = "[column names]\na b\n[data]\n1 2\nnope 3\n123456789012345678901\n4 5\n";
    let parser = Parser::new(ParseOptions {
        max_line_bytes: 20,
        ..ParseOptions::default()
    });
    let mut accepted = Vec::new();
    let report = parser
        .parse_bufread_recovering(BufReader::new(Cursor::new(source)), |sample| {
            accepted.push(sample.values()[0]);
        })
        .expect("recovery continues");

    assert_eq!(accepted, vec![1.0, 4.0]);
    assert_eq!(report.rows, 2);
    assert!(matches!(
        report.issues[0].kind,
        ParseIssueKind::InvalidNumber { column: 1, .. }
    ));
    assert!(matches!(
        report.issues[1].kind,
        ParseIssueKind::LineTooLong { limit: 20 }
    ));
}

#[test]
fn streaming_strict_mode_stops_at_the_first_bad_row() {
    let source = "[column names]\na\n[data]\n1\nbad\n2\n";
    let mut accepted = 0;
    let error = Parser::default()
        .parse_bufread(BufReader::new(Cursor::new(source)), |_| accepted += 1)
        .expect_err("strict parser rejects malformed row");

    assert_eq!(accepted, 1);
    assert!(matches!(
        error,
        ParseError::InvalidNumber {
            line: 5,
            column: 1,
            ..
        }
    ));
}

#[test]
fn streaming_handles_many_rows_with_a_reused_callback_buffer() {
    let mut source = String::from("[column names]\na\n[data]\n");
    for value in 0..50_000 {
        writeln!(source, "{value}").expect("writing to a String cannot fail");
    }

    let mut sum = 0.0;
    let report = Parser::default()
        .parse_bufread(BufReader::new(Cursor::new(source)), |sample| {
            sum += sample.value(0).expect("single column");
        })
        .expect("large stream parses");

    assert_eq!(report.rows, 50_000);
    assert!((sum - 1_249_975_000.0).abs() < f64::EPSILON);
}

#[test]
fn streaming_recovery_bounds_retained_diagnostics() {
    let source = "[column names]\na\n[data]\nbad\nstill-bad\n";
    let parser = Parser::new(ParseOptions {
        max_issues: 1,
        ..ParseOptions::default()
    });
    let error = parser
        .parse_bufread_recovering(BufReader::new(Cursor::new(source)), |_| {})
        .expect_err("diagnostic collection must be bounded");
    assert!(matches!(
        error,
        ParseError::IssueLimit { line: 5, limit: 1 }
    ));
}
