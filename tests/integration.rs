use std::io::Cursor;

use proptest::prelude::*;
use racelogic_vbo::{
    packed_minutes_to_degrees, CoordinateAxis, ParseError, ParseIssueKind, ParseOptions, Parser,
    Telemetry,
};

const BASIC: &str = include_str!("fixtures/basic.vbo");

#[test]
fn parses_documented_shape_and_decodes_telemetry() {
    let vbo = Parser::default().parse_str(BASIC).expect("fixture parses");
    assert_eq!(vbo.row_count(), 3);
    assert_eq!(vbo.column_count(), 8);
    assert_eq!(vbo.channels[4].unit.as_deref(), Some("kmh"));
    assert_eq!(vbo.channel_index("vertical_velocity"), None);
    assert_eq!(vbo.satellite_quality(0).expect("sats").satellites, 9);
    assert!(vbo.satellite_quality(0).expect("sats").dgps);
    assert!(
        (vbo.time_seconds(0).expect("time") - (16.0 * 3600.0 + 22.0 * 60.0 + 35.4)).abs() < 1e-9
    );
    let point = vbo.geo_point(0).expect("coordinates");
    assert!(point.latitude_deg > 31.0);
    assert!(
        point.longitude_deg < 0.0,
        "positive VBO longitude means west"
    );
    let metrics = vbo.analyse();
    assert_eq!(metrics.samples, 3);
    assert!(metrics.distance_metres.expect("distance") > 0.0);
    assert_eq!(metrics.max_speed_kmh, Some(0.25));
}

#[test]
fn strict_mode_is_transactional_on_bad_rows() {
    let bad = BASIC.replacen("137 162235.90", "137 nope", 1);
    let error = Parser::default()
        .parse_reader(Cursor::new(bad))
        .expect_err("must reject");
    assert!(matches!(error, ParseError::InvalidNumber { .. }));
}

#[test]
fn recovery_omits_bad_rows_and_reports_them() {
    let bad = BASIC.replacen("137 162235.90", "137 nope", 1);
    let report = Parser::default()
        .parse_reader_recovering(Cursor::new(bad))
        .expect("recover");
    assert_eq!(report.vbo.row_count(), 2);
    assert!(matches!(
        report.issues[0].kind,
        ParseIssueKind::InvalidNumber { .. }
    ));
}

#[test]
fn coordinate_axes_follow_racelogic_sign_convention() {
    let lat = packed_minutes_to_degrees(3_119.099_73, CoordinateAxis::Latitude).expect("latitude");
    let long = packed_minutes_to_degrees(58.492_77, CoordinateAxis::Longitude).expect("longitude");
    assert!((lat - 31.318_328_833).abs() < 1e-8);
    assert!(long < 0.0);
}

#[test]
fn malformed_inputs_are_bounded_and_precisely_located() {
    let options = ParseOptions {
        max_line_bytes: 20,
        ..ParseOptions::default()
    };
    let error = Parser::new(options)
        .parse_str("[column names]\na\n[data]\n123456789012345678901\n")
        .expect_err("line cap");
    assert!(matches!(error, ParseError::LineTooLong { line: 4, .. }));
}

#[test]
fn telemetry_converts_declared_speed_units() {
    let source = "[channel units]\nhhmmss kts\n[column names]\ntime velocity\n[data]\n235959.0 10\n000001.0 20\n";
    let metrics = Parser::default()
        .parse_str(source)
        .expect("parse")
        .analyse();
    assert_eq!(metrics.duration_seconds, Some(2.0));
    assert_eq!(metrics.max_speed_kmh, Some(37.04));
}

proptest! {
    #[test]
    fn never_accepts_invalid_minutes(degrees in 0_u16..180_u16, minutes in 60.0_f64..100.0) {
        let packed = f64::from(degrees) * 100.0 + minutes;
        prop_assert_eq!(packed_minutes_to_degrees(packed, CoordinateAxis::Longitude), None);
    }
}
