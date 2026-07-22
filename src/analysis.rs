//! Deterministic building blocks for lap timing and uniformly sampled telemetry channels.
//!
//! This module deliberately operates on small, explicit data types rather than assuming a
//! particular VBO channel layout. That makes it suitable for native VBOX channels, CAN data, and
//! derived signals alike. Timestamps are seconds on a strictly increasing timeline; callers that
//! use VBO UTC time should unwrap a midnight rollover before invoking these APIs.

use thiserror::Error;

use crate::GeoPoint;

/// Upper bound for samples created by one [`TimeSeries::resample_uniform`] call.
pub const MAX_RESAMPLED_SAMPLES: usize = 10_000_000;
const MAX_RESAMPLED_STEPS: f64 = 10_000_000.0;

/// A timestamped geographic position used for gate and lap detection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedPoint {
    /// Monotonically increasing seconds on a single timeline.
    pub time_seconds: f64,
    pub point: GeoPoint,
}

/// Which direction may cross a gate.
///
/// The sign is relative to the directed gate segment from [`Gate::start`] to [`Gate::end`].
/// `NegativeToPositive` therefore means the vehicle moves from the segment's right-hand side to
/// its left-hand side in conventional longitude/latitude coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GateDirection {
    /// Accept either direction.
    #[default]
    Either,
    /// Accept only crossings whose signed side changes from negative to positive.
    NegativeToPositive,
    /// Accept only crossings whose signed side changes from positive to negative.
    PositiveToNegative,
}

/// A finite geographic line segment used as a timing gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    pub start: GeoPoint,
    pub end: GeoPoint,
    pub direction: GateDirection,
}

impl Gate {
    /// Builds a bidirectional gate.
    #[must_use]
    pub const fn bidirectional(start: GeoPoint, end: GeoPoint) -> Self {
        Self {
            start,
            end,
            direction: GateDirection::Either,
        }
    }
}

/// A named intermediate timing gate, in the order it occurs around a lap.
#[derive(Clone, Debug, PartialEq)]
pub struct SectorGate {
    pub name: String,
    pub gate: Gate,
}

/// Settings for [`detect_laps`].
#[derive(Clone, Debug, PartialEq)]
pub struct LapConfig {
    pub start_finish: Gate,
    /// Intermediate gates in their expected traversal order.
    pub sectors: Vec<SectorGate>,
}

/// One completed sector within a lap.
#[derive(Clone, Debug, PartialEq)]
pub struct SectorTime {
    pub name: String,
    /// Seconds from the lap start to this gate, if it was crossed in order.
    pub crossing_elapsed_seconds: Option<f64>,
    /// Sector duration from the preceding timing boundary, if it was crossed in order.
    pub duration_seconds: Option<f64>,
}

/// A lap bounded by two crossings of a start/finish gate.
#[derive(Clone, Debug, PartialEq)]
pub struct Lap {
    pub started_at_seconds: f64,
    pub finished_at_seconds: f64,
    pub duration_seconds: f64,
    /// One value for every configured sector, including a missing value for an uncrossed gate.
    pub sectors: Vec<SectorTime>,
}

/// A validated time-series channel. Values may be non-finite to represent missing observations.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeSeries {
    timestamps: Vec<f64>,
    values: Vec<f64>,
}

impl TimeSeries {
    /// Creates a time series with strictly increasing, finite timestamps.
    ///
    /// Values are intentionally not rejected: `NaN` and infinities represent missing samples and
    /// produce `None` during interpolation. Smoothing ignores them.
    pub fn new(timestamps: Vec<f64>, values: Vec<f64>) -> Result<Self, AnalysisError> {
        validate_series(&timestamps, &values)?;
        Ok(Self { timestamps, values })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.timestamps.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    #[must_use]
    pub fn timestamps(&self) -> &[f64] {
        &self.timestamps
    }

    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Linearly interpolates a finite observation at `timestamp`.
    ///
    /// There is no extrapolation. A query outside the recorded range, or bracketed by a missing
    /// observation, returns `Ok(None)`.
    pub fn interpolate(&self, timestamp: f64) -> Result<Option<f64>, AnalysisError> {
        if !timestamp.is_finite() {
            return Err(AnalysisError::InvalidQueryTimestamp);
        }
        let result = self
            .timestamps
            .binary_search_by(|candidate| candidate.total_cmp(&timestamp));
        match result {
            Ok(index) => Ok(self.values[index].is_finite().then_some(self.values[index])),
            Err(0) => Ok(None),
            Err(upper) if upper == self.timestamps.len() => Ok(None),
            Err(upper) => {
                let lower = upper - 1;
                let lower_value = self.values[lower];
                let upper_value = self.values[upper];
                if !(lower_value.is_finite() && upper_value.is_finite()) {
                    return Ok(None);
                }
                let start = self.timestamps[lower];
                let span = self.timestamps[upper] - start;
                let fraction = (timestamp - start) / span;
                let value = lower_value + (upper_value - lower_value) * fraction;
                Ok(value.is_finite().then_some(value))
            }
        }
    }

    /// Evaluates this channel at every supplied timestamp.
    pub fn align_to(&self, timestamps: &[f64]) -> Result<AlignedChannel, AnalysisError> {
        let mut values = Vec::with_capacity(timestamps.len());
        for &timestamp in timestamps {
            values.push(self.interpolate(timestamp)?);
        }
        Ok(AlignedChannel {
            timestamps: timestamps.to_vec(),
            values,
        })
    }

    /// Produces a uniform time grid from `start_seconds` through the last grid point no later
    /// than `end_seconds`. The end is included when it falls exactly on the grid.
    pub fn resample_uniform(
        &self,
        start_seconds: f64,
        end_seconds: f64,
        interval_seconds: f64,
    ) -> Result<AlignedChannel, AnalysisError> {
        if !(start_seconds.is_finite()
            && end_seconds.is_finite()
            && interval_seconds.is_finite()
            && start_seconds <= end_seconds
            && interval_seconds > 0.0)
        {
            return Err(AnalysisError::InvalidResamplingInterval);
        }
        let steps = ((end_seconds - start_seconds) / interval_seconds).floor();
        if steps >= MAX_RESAMPLED_STEPS {
            return Err(AnalysisError::OutputLimit {
                limit: MAX_RESAMPLED_SAMPLES,
            });
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = steps as usize + 1;
        let mut timestamps = Vec::with_capacity(count);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            #[allow(clippy::cast_precision_loss)]
            let timestamp = (index as f64).mul_add(interval_seconds, start_seconds);
            timestamps.push(timestamp);
            values.push(self.interpolate(timestamp)?);
        }
        Ok(AlignedChannel { timestamps, values })
    }

    /// Applies a trailing moving mean over up to `window` samples.
    ///
    /// Non-finite source values are ignored. A window containing no finite observations is
    /// represented by `NaN` in the output, preserving the source timestamp alignment.
    pub fn smooth_moving_mean(&self, window: usize) -> Result<Self, AnalysisError> {
        if window == 0 {
            return Err(AnalysisError::InvalidSmoothingWindow);
        }
        let mut values = Vec::with_capacity(self.values.len());
        let mut sum = 0.0;
        let mut finite_count = 0usize;
        for (index, &value) in self.values.iter().enumerate() {
            if value.is_finite() {
                sum += value;
                finite_count += 1;
            }
            if index >= window {
                let outgoing = self.values[index - window];
                if outgoing.is_finite() {
                    sum -= outgoing;
                    finite_count -= 1;
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let mean = (finite_count > 0)
                .then_some(sum / finite_count as f64)
                .filter(|mean| mean.is_finite())
                .unwrap_or(f64::NAN);
            values.push(mean);
        }
        Ok(Self {
            timestamps: self.timestamps.clone(),
            values,
        })
    }
}

/// One channel aligned to a caller-supplied or uniform time grid.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignedChannel {
    pub timestamps: Vec<f64>,
    /// Missing, out-of-range, or invalidly bracketed observations are `None`.
    pub values: Vec<Option<f64>>,
}

/// Aligns multiple channels to one common timestamp grid using linear interpolation.
pub fn align_channels(
    timestamps: &[f64],
    channels: &[&TimeSeries],
) -> Result<Vec<AlignedChannel>, AnalysisError> {
    channels
        .iter()
        .map(|channel| channel.align_to(timestamps))
        .collect()
}

/// Finds completed laps and sector times from a chronologically ordered GPS trace.
///
/// A gate only triggers when consecutive points lie strictly on opposite sides of its line and
/// their interpolated path crosses the finite gate segment. Touching or travelling along a gate
/// is therefore deliberately ignored, avoiding duplicate events from stationary GPS samples.
pub fn detect_laps(samples: &[TimedPoint], config: &LapConfig) -> Result<Vec<Lap>, AnalysisError> {
    validate_gate(&config.start_finish, "start/finish")?;
    for sector in &config.sectors {
        validate_gate(&sector.gate, &sector.name)?;
    }
    validate_points(samples)?;

    let mut laps = Vec::new();
    let mut lap_start = None;
    let mut sector_crossings = vec![None; config.sectors.len()];
    let mut next_sector = 0usize;

    for (index, pair) in samples.windows(2).enumerate() {
        let before = pair[0];
        let after = pair[1];
        let mut events = Vec::with_capacity(config.sectors.len() + 1);
        if let Some(fraction) =
            gate_crossing_fraction(before.point, after.point, config.start_finish)
        {
            events.push((fraction, GateEvent::StartFinish));
        }
        for (sector_index, sector) in config.sectors.iter().enumerate() {
            if let Some(fraction) = gate_crossing_fraction(before.point, after.point, sector.gate) {
                events.push((fraction, GateEvent::Sector(sector_index)));
            }
        }
        events.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| event_order(left.1).cmp(&event_order(right.1)))
        });

        for (fraction, event) in events {
            let crossing = interpolate_time(before.time_seconds, after.time_seconds, fraction);
            match event {
                GateEvent::StartFinish => {
                    if let Some(start) = lap_start {
                        if crossing > start {
                            laps.push(build_lap(start, crossing, &sector_crossings, config));
                        }
                    }
                    lap_start = Some(crossing);
                    sector_crossings.fill(None);
                    next_sector = 0;
                }
                GateEvent::Sector(sector_index)
                    if lap_start.is_some() && sector_index == next_sector =>
                {
                    sector_crossings[sector_index] = Some(crossing);
                    next_sector += 1;
                }
                GateEvent::Sector(_) => {}
            }
        }
        debug_assert!(index + 1 < samples.len());
    }
    Ok(laps)
}

#[derive(Clone, Copy)]
enum GateEvent {
    StartFinish,
    Sector(usize),
}

const fn event_order(event: GateEvent) -> usize {
    match event {
        GateEvent::StartFinish => 0,
        GateEvent::Sector(index) => index + 1,
    }
}

fn build_lap(start: f64, finish: f64, crossings: &[Option<f64>], config: &LapConfig) -> Lap {
    let mut previous = start;
    let sectors = config
        .sectors
        .iter()
        .zip(crossings)
        .map(|(sector, crossing)| {
            let duration_seconds = crossing.map(|time| {
                let duration = time - previous;
                previous = time;
                duration
            });
            SectorTime {
                name: sector.name.clone(),
                crossing_elapsed_seconds: crossing.map(|time| time - start),
                duration_seconds,
            }
        })
        .collect();
    Lap {
        started_at_seconds: start,
        finished_at_seconds: finish,
        duration_seconds: finish - start,
        sectors,
    }
}

fn interpolate_time(start: f64, end: f64, fraction: f64) -> f64 {
    (end - start).mul_add(fraction, start)
}

fn gate_crossing_fraction(before: GeoPoint, after: GeoPoint, gate: Gate) -> Option<f64> {
    let latitude_scale = gate.start.latitude_deg.to_radians().cos();
    let to_local = |point: GeoPoint| {
        (
            (point.longitude_deg - gate.start.longitude_deg) * latitude_scale,
            point.latitude_deg - gate.start.latitude_deg,
        )
    };
    let gate_end = to_local(gate.end);
    let before = to_local(before);
    let after = to_local(after);
    let side_before = cross(gate_end, before);
    let side_after = cross(gate_end, after);
    if !(side_before.is_finite()
        && side_after.is_finite()
        && ((side_before < 0.0 && side_after > 0.0) || (side_before > 0.0 && side_after < 0.0)))
    {
        return None;
    }
    if !matches_direction(side_before, side_after, gate.direction) {
        return None;
    }
    let fraction = side_before.abs() / (side_before.abs() + side_after.abs());
    let crossing = (
        before.0 + (after.0 - before.0) * fraction,
        before.1 + (after.1 - before.1) * fraction,
    );
    let gate_length_squared = dot(gate_end, gate_end);
    if !(gate_length_squared.is_finite() && gate_length_squared > f64::EPSILON) {
        return None;
    }
    let gate_fraction = dot(crossing, gate_end) / gate_length_squared;
    ((0.0..=1.0).contains(&gate_fraction) && fraction.is_finite()).then_some(fraction)
}

fn matches_direction(before: f64, after: f64, direction: GateDirection) -> bool {
    match direction {
        GateDirection::Either => true,
        GateDirection::NegativeToPositive => before < 0.0 && after > 0.0,
        GateDirection::PositiveToNegative => before > 0.0 && after < 0.0,
    }
}

fn cross(left: (f64, f64), right: (f64, f64)) -> f64 {
    left.0 * right.1 - left.1 * right.0
}

fn dot(left: (f64, f64), right: (f64, f64)) -> f64 {
    left.0 * right.0 + left.1 * right.1
}

fn validate_series(timestamps: &[f64], values: &[f64]) -> Result<(), AnalysisError> {
    if timestamps.len() != values.len() {
        return Err(AnalysisError::MismatchedLengths {
            timestamps: timestamps.len(),
            values: values.len(),
        });
    }
    validate_timestamps(timestamps)
}

fn validate_points(samples: &[TimedPoint]) -> Result<(), AnalysisError> {
    let mut timestamps = Vec::with_capacity(samples.len());
    for (index, sample) in samples.iter().enumerate() {
        timestamps.push(sample.time_seconds);
        if !valid_point(sample.point) {
            return Err(AnalysisError::InvalidPoint { index });
        }
    }
    validate_timestamps(&timestamps)
}

fn validate_timestamps(timestamps: &[f64]) -> Result<(), AnalysisError> {
    for (index, &timestamp) in timestamps.iter().enumerate() {
        if !timestamp.is_finite() {
            return Err(AnalysisError::InvalidTimestamp { index });
        }
        if index > 0 && timestamp <= timestamps[index - 1] {
            return Err(AnalysisError::NonMonotonicTimestamp { index });
        }
    }
    Ok(())
}

fn validate_gate(gate: &Gate, name: &str) -> Result<(), AnalysisError> {
    if !(valid_point(gate.start) && valid_point(gate.end)) || gate.start == gate.end {
        return Err(AnalysisError::InvalidGate {
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn valid_point(point: GeoPoint) -> bool {
    point.latitude_deg.is_finite()
        && point.longitude_deg.is_finite()
        && (-90.0..=90.0).contains(&point.latitude_deg)
        && (-180.0..=180.0).contains(&point.longitude_deg)
}

/// A malformed input or impossible analysis request.
#[derive(Debug, Error, PartialEq)]
pub enum AnalysisError {
    #[error("timestamp and value counts differ ({timestamps} timestamps, {values} values)")]
    MismatchedLengths { timestamps: usize, values: usize },
    #[error("timestamp at index {index} is not finite")]
    InvalidTimestamp { index: usize },
    #[error("timestamp at index {index} is not strictly greater than its predecessor")]
    NonMonotonicTimestamp { index: usize },
    #[error("query timestamp is not finite")]
    InvalidQueryTimestamp,
    #[error("point at index {index} is not a finite WGS84 coordinate")]
    InvalidPoint { index: usize },
    #[error("gate `{name}` is not a non-zero finite WGS84 segment")]
    InvalidGate { name: String },
    #[error("resampling requires finite start/end, start <= end, and a positive finite interval")]
    InvalidResamplingInterval,
    #[error("smoothing window must be at least one sample")]
    InvalidSmoothingWindow,
    #[error("resampling would create more than {limit} samples")]
    OutputLimit { limit: usize },
}
