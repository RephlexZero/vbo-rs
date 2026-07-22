//! A fast, fault-aware parser and telemetry toolkit for Racelogic VBOX `.vbo` files.
//!
//! The parser reads input once, performs no per-field string allocation, and stores data in a
//! contiguous row-major `Vec<f64>`. See [`Parser`] for ingestion and [`Telemetry`] for analysis.

#![forbid(unsafe_code)]

mod analysis;
mod parser;
mod telemetry;
mod types;

pub use analysis::{
    align_channels, detect_laps, AlignedChannel, AnalysisError, Gate, GateDirection, Lap,
    LapConfig, SectorGate, SectorTime, TimeSeries, TimedPoint, MAX_RESAMPLED_SAMPLES,
};
pub use parser::{parse_path, ParseOptions, Parser};
pub use telemetry::{
    packed_minutes_to_degrees, CoordinateAxis, GeoPoint, SatelliteQuality, SessionMetrics,
    Telemetry,
};
pub use types::{
    Channel, Header, ParseError, ParseIssue, ParseIssueKind, ParseReport, SampleRef, StreamReport,
    StreamSample, Vbo,
};
