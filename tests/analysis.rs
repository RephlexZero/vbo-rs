use racelogic_vbo::{
    align_channels, detect_laps, AnalysisError, Gate, GateDirection, GeoPoint, LapConfig,
    SectorGate, TimeSeries, TimedPoint,
};

fn point(latitude_deg: f64, longitude_deg: f64) -> GeoPoint {
    GeoPoint {
        latitude_deg,
        longitude_deg,
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::EPSILON,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn detects_interpolated_laps_and_ordered_sectors() {
    let start_finish = Gate {
        start: point(-1.0, 0.0),
        end: point(1.0, 0.0),
        direction: GateDirection::PositiveToNegative,
    };
    let sector = SectorGate {
        name: "Back straight".to_owned(),
        gate: Gate::bidirectional(point(-1.0, 1.0), point(1.0, 1.0)),
    };
    let trace = [
        TimedPoint {
            time_seconds: 0.0,
            point: point(0.0, -0.5),
        },
        TimedPoint {
            time_seconds: 2.0,
            point: point(0.0, 0.5),
        },
        TimedPoint {
            time_seconds: 4.0,
            point: point(0.0, 1.5),
        },
        TimedPoint {
            time_seconds: 6.0,
            point: point(0.0, -0.5),
        },
        TimedPoint {
            time_seconds: 8.0,
            point: point(0.0, 0.5),
        },
        TimedPoint {
            time_seconds: 10.0,
            point: point(0.0, 1.5),
        },
        TimedPoint {
            time_seconds: 12.0,
            point: point(0.0, -0.5),
        },
    ];
    let laps = detect_laps(
        &trace,
        &LapConfig {
            start_finish,
            sectors: vec![sector],
        },
    )
    .unwrap();

    assert_eq!(laps.len(), 1);
    assert_close(laps[0].started_at_seconds, 1.0);
    assert_close(laps[0].finished_at_seconds, 7.0);
    assert_close(laps[0].duration_seconds, 6.0);
    assert_eq!(laps[0].sectors[0].crossing_elapsed_seconds, Some(2.0));
    assert_eq!(laps[0].sectors[0].duration_seconds, Some(2.0));
}

#[test]
fn gate_direction_and_gate_touches_are_deterministic() {
    let gate = Gate {
        start: point(-1.0, 0.0),
        end: point(1.0, 0.0),
        direction: GateDirection::PositiveToNegative,
    };
    let config = LapConfig {
        start_finish: gate,
        sectors: Vec::new(),
    };
    let reverse_trace = [
        TimedPoint {
            time_seconds: 0.0,
            point: point(0.0, 0.5),
        },
        TimedPoint {
            time_seconds: 1.0,
            point: point(0.0, -0.5),
        },
        TimedPoint {
            time_seconds: 2.0,
            point: point(0.0, 0.0),
        },
        TimedPoint {
            time_seconds: 3.0,
            point: point(0.0, -0.5),
        },
    ];
    assert!(detect_laps(&reverse_trace, &config).unwrap().is_empty());

    let forward_trace = [
        TimedPoint {
            time_seconds: 0.0,
            point: point(0.0, -0.5),
        },
        TimedPoint {
            time_seconds: 2.0,
            point: point(0.0, 0.5),
        },
        TimedPoint {
            time_seconds: 4.0,
            point: point(0.0, -0.5),
        },
        TimedPoint {
            time_seconds: 6.0,
            point: point(0.0, 0.5),
        },
    ];
    assert_eq!(detect_laps(&forward_trace, &config).unwrap().len(), 1);
}

#[test]
fn rejects_invalid_gates_and_non_monotonic_traces() {
    let invalid = LapConfig {
        start_finish: Gate::bidirectional(point(0.0, 0.0), point(0.0, 0.0)),
        sectors: Vec::new(),
    };
    assert!(matches!(
        detect_laps(&[], &invalid),
        Err(AnalysisError::InvalidGate { .. })
    ));
    let config = LapConfig {
        start_finish: Gate::bidirectional(point(-1.0, 0.0), point(1.0, 0.0)),
        sectors: Vec::new(),
    };
    let trace = [
        TimedPoint {
            time_seconds: 1.0,
            point: point(0.0, -1.0),
        },
        TimedPoint {
            time_seconds: 1.0,
            point: point(0.0, 1.0),
        },
    ];
    assert_eq!(
        detect_laps(&trace, &config),
        Err(AnalysisError::NonMonotonicTimestamp { index: 1 })
    );
}

#[test]
fn interpolates_resamples_and_aligns_channels_without_extrapolation() {
    let series = TimeSeries::new(vec![0.0, 2.0, 4.0], vec![0.0, 20.0, 40.0]).unwrap();
    assert_eq!(series.interpolate(1.0).unwrap(), Some(10.0));
    assert_eq!(series.interpolate(-1.0).unwrap(), None);
    let resampled = series.resample_uniform(0.0, 4.0, 1.0).unwrap();
    assert_eq!(resampled.timestamps, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        resampled.values,
        vec![Some(0.0), Some(10.0), Some(20.0), Some(30.0), Some(40.0)]
    );

    let secondary = TimeSeries::new(vec![1.0, 3.0], vec![5.0, 9.0]).unwrap();
    let aligned = align_channels(&[0.0, 1.0, 2.0, 3.0], &[&series, &secondary]).unwrap();
    assert_eq!(
        aligned[1].values,
        vec![None, Some(5.0), Some(7.0), Some(9.0)]
    );
}

#[test]
fn missing_values_do_not_poison_smoothing_or_interpolation() {
    let series = TimeSeries::new(vec![0.0, 1.0, 2.0, 3.0], vec![1.0, f64::NAN, 5.0, 7.0]).unwrap();
    assert_eq!(series.interpolate(1.5).unwrap(), None);
    let smoothed = series.smooth_moving_mean(2).unwrap();
    assert_eq!(smoothed.values(), &[1.0, 1.0, 5.0, 6.0]);
    assert_eq!(
        TimeSeries::new(vec![0.0], vec![1.0, 2.0]),
        Err(AnalysisError::MismatchedLengths {
            timestamps: 1,
            values: 2
        })
    );
}
