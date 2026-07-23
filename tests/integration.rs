use std::io::Cursor;

use proptest::prelude::*;
use racelogic_vbo::{
    packed_minutes_to_degrees, CoordinateAxis, CoordinateFormat, ParseError, ParseIssueKind,
    ParseOptions, Parser, Telemetry,
};

const BASIC: &str = include_str!("fixtures/basic.vbo");
const CONTINUOUS_MINUTES: &str = include_str!("fixtures/continuous_minutes.vbo");

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
    assert_eq!(
        vbo.coordinate_format(),
        CoordinateFormat::PackedDegreesMinutes
    );
    let metrics = vbo.analyse();
    assert_eq!(metrics.samples, 3);
    assert!(metrics.distance_metres.expect("distance") > 0.0);
    assert_eq!(metrics.max_speed_kmh, Some(0.25));
}

/// Some hardware — observed on a Video VBOX HD2 dashcam unit — logs `lat`/`long` as a single
/// continuous value in minutes rather than the documented packed `DDMM.MMMM` format. Every row
/// in this real-world fixture fails to decode under the packed convention (the minutes remainder
/// is always over 60), so the recording must be auto-detected as `ContinuousMinutes` for its
/// coordinates, distance, and speed to resolve at all.
#[test]
fn auto_detects_continuous_minutes_hardware() {
    let vbo = Parser::default()
        .parse_str(CONTINUOUS_MINUTES)
        .expect("fixture parses");
    assert_eq!(vbo.coordinate_format(), CoordinateFormat::ContinuousMinutes);

    let point = vbo.geo_point(0).expect("coordinates");
    // Real-world reference: Oulton Park Circuit, Cheshire, UK.
    assert!((point.latitude_deg - 53.178_876).abs() < 1e-3);
    assert!((point.longitude_deg - -2.612_849).abs() < 1e-3);

    let metrics = vbo.analyse();
    assert!(metrics.distance_metres.expect("distance") > 0.0);
    assert!(metrics.max_speed_kmh.expect("speed") > 0.0);
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

#[test]
fn telemetry_summarises_inertial_turn_and_application_defined_channels() {
    let source = "[channel units]\nhhmmss g m/s2 g rad/s ft Nm\n[column names]\ntime long_acc lat-acc vertical_acc yaw_rate radius_of_turn CAN_Engine_Torque\n[data]\n120000.0 1 3 -0.5 3.141592653589793 100 250\n120001.0 2 4 0.5 0 50 270\n120002.0 3 5 1.0 -1.5707963267948966 -10 260\n";
    let metrics = Parser::default()
        .parse_str(source)
        .expect("parse")
        .analyse();

    let longitudinal = metrics
        .longitudinal_acceleration
        .as_ref()
        .expect("longitudinal acceleration");
    assert_eq!(longitudinal.unit.as_deref(), Some("m/s²"));
    assert_eq!(longitudinal.samples, 3);
    assert!((longitudinal.maximum - 3.0 * 9.806_65).abs() < 1e-12);
    assert!(
        (metrics
            .lateral_acceleration
            .as_ref()
            .expect("lateral acceleration")
            .minimum
            - 3.0)
            .abs()
            < 1e-12
    );
    assert!(
        (metrics
            .vertical_acceleration
            .as_ref()
            .expect("vertical acceleration")
            .maximum
            - 9.806_65)
            .abs()
            < 1e-12
    );
    let yaw_rate = metrics.yaw_rate.as_ref().expect("yaw rate");
    assert_eq!(yaw_rate.unit.as_deref(), Some("deg/s"));
    assert!((yaw_rate.maximum - 180.0).abs() < 1e-10);
    let radius = metrics.radius_of_turn.as_ref().expect("turn radius");
    assert_eq!(radius.unit.as_deref(), Some("m"));
    assert_eq!(radius.samples, 2, "negative physical radii are omitted");
    assert!((radius.maximum - 30.48).abs() < 1e-12);

    let can = metrics
        .numeric_channels
        .iter()
        .find(|summary| summary.channel == "CAN_Engine_Torque")
        .expect("application-defined numeric channel");
    assert_eq!(can.unit.as_deref(), Some("Nm"));
    assert_eq!((can.minimum, can.maximum, can.mean), (250.0, 270.0, 260.0));
}

#[test]
fn inertial_conversion_never_guesses_an_undeclared_unit() {
    let source = "[column names]\nlongitudinal_acceleration CAN_Fuel\n[data]\n1.0 20\n2.0 22\n";
    let metrics = Parser::default()
        .parse_str(source)
        .expect("parse")
        .analyse();
    assert_eq!(metrics.longitudinal_acceleration, None);
    assert_eq!(metrics.numeric_channels.len(), 2);
}

proptest! {
    #[test]
    fn never_accepts_invalid_minutes(degrees in 0_u16..180_u16, minutes in 60.0_f64..100.0) {
        let packed = f64::from(degrees) * 100.0 + minutes;
        prop_assert_eq!(packed_minutes_to_degrees(packed, CoordinateAxis::Longitude), None);
    }
}
