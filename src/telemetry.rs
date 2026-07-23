use crate::{
    types::{normalise, Channel},
    Vbo,
};

/// Axis semantics for Racelogic's packed-minutes coordinate encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateAxis {
    Latitude,
    Longitude,
}

/// Which real-world Racelogic `lat`/`long` convention a recording uses.
///
/// Classic VBOX/VBOX3 loggers use the documented packed `DDMM.MMMM` format (see
/// `docs/VBO_FORMAT.md`), but some newer hardware — observed on a Video VBOX HD2 dashcam unit —
/// logs `lat`/`long` as plain continuous minutes instead. [`Vbo::coordinate_format`] detects this
/// once per recording, so [`Telemetry::geo_point`] and [`Telemetry::analyse`] decode every row
/// consistently without re-detecting per call.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoordinateFormat {
    /// `DDMM.MMMM` / `DDDMM.MMMM`: degrees and minutes packed into one field.
    #[default]
    PackedDegreesMinutes,
    /// A single continuous value in minutes (degrees × 60 + minutes).
    ContinuousMinutes,
}

/// Detects which [`CoordinateFormat`] a recording's `lat`/`long` channels use.
///
/// A genuine packed-format recording decodes almost every row; if the packed convention fails
/// for the majority of rows with present coordinates, the hardware logs continuous minutes
/// instead. Runs once per recording, before any [`Vbo`] exists, so it addresses `values`
/// positionally rather than through [`Vbo::value`].
#[must_use]
pub(crate) fn detect_coordinate_format(
    channels: &[Channel],
    values: &[f64],
    rows: usize,
) -> CoordinateFormat {
    let width = channels.len();
    if width == 0 || rows == 0 {
        return CoordinateFormat::PackedDegreesMinutes;
    }
    let Some(lat_column) = find_channel(channels, &["lat", "latitude"]) else {
        return CoordinateFormat::PackedDegreesMinutes;
    };
    let Some(long_column) = find_channel(channels, &["long", "longitude", "lon"]) else {
        return CoordinateFormat::PackedDegreesMinutes;
    };

    let mut present = 0usize;
    let mut packed_valid = 0usize;
    for row in 0..rows {
        let latitude = values[row * width + lat_column];
        let longitude = values[row * width + long_column];
        if !latitude.is_finite() || !longitude.is_finite() {
            continue;
        }
        present += 1;
        let packed = packed_minutes_to_degrees(latitude, CoordinateAxis::Latitude).is_some()
            && packed_minutes_to_degrees(longitude, CoordinateAxis::Longitude).is_some();
        if packed {
            packed_valid += 1;
        }
    }

    if present > 0 && packed_valid * 2 < present {
        CoordinateFormat::ContinuousMinutes
    } else {
        CoordinateFormat::PackedDegreesMinutes
    }
}

fn find_channel(channels: &[Channel], aliases: &[&str]) -> Option<usize> {
    channels.iter().position(|channel| {
        aliases
            .iter()
            .any(|alias| normalise(&channel.name) == *alias)
    })
}

/// Decodes a continuous-minutes `lat`/`long` pair. See [`CoordinateFormat::ContinuousMinutes`].
fn continuous_minutes_to_degrees(
    latitude_minutes: f64,
    longitude_minutes: f64,
) -> Option<GeoPoint> {
    if !latitude_minutes.is_finite() || !longitude_minutes.is_finite() {
        return None;
    }
    let latitude_deg = latitude_minutes / 60.0;
    // Racelogic's legacy sign convention applies here too: positive VBO longitude means west.
    let longitude_deg = -longitude_minutes / 60.0;
    (latitude_deg.abs() <= 90.0 && longitude_deg.abs() <= 180.0).then_some(GeoPoint {
        latitude_deg,
        longitude_deg,
    })
}

/// A WGS84-compatible geographic point in conventional signed decimal degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoPoint {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}

/// Satellite and trigger flags decoded from the VBO `sats` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SatelliteQuality {
    pub satellites: u8,
    pub dgps: bool,
    pub brake_trigger: bool,
}

/// Summary of finite samples from a numeric channel.
///
/// Values retain the channel's source unit unless the summary is exposed through one of the
/// unit-normalised inertial or turn fields in [`SessionMetrics`].  Every VBO data column is
/// represented by [`SessionMetrics::numeric_channels`], which lets callers analyse CAN-derived
/// channels without relying on a device- or supplier-specific channel naming convention.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct NumericChannelSummary {
    /// Name declared in `[column names]`.
    pub channel: String,
    /// Unit declared in `[channel units]`, or the canonical unit of a converted summary.
    pub unit: Option<String>,
    /// Number of finite samples included in the summary.
    pub samples: usize,
    pub minimum: f64,
    pub maximum: f64,
    pub mean: f64,
}

/// Deterministic summary calculations over a VBO session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionMetrics {
    pub samples: usize,
    pub duration_seconds: Option<f64>,
    pub distance_metres: Option<f64>,
    pub mean_speed_kmh: Option<f64>,
    pub max_speed_kmh: Option<f64>,
    pub peak_acceleration_mps2: Option<f64>,
    pub peak_braking_mps2: Option<f64>,
    pub timing_anomalies: usize,
    /// Direct longitudinal accelerometer samples, normalised to `m/s²` when both a recognised
    /// channel alias and unit are present.
    pub longitudinal_acceleration: Option<NumericChannelSummary>,
    /// Direct lateral accelerometer samples, normalised to `m/s²`.
    pub lateral_acceleration: Option<NumericChannelSummary>,
    /// Direct vertical accelerometer samples, normalised to `m/s²`.
    pub vertical_acceleration: Option<NumericChannelSummary>,
    /// Yaw-rate samples, normalised to `deg/s`.
    pub yaw_rate: Option<NumericChannelSummary>,
    /// Turn-radius samples, normalised to metres. Negative values are ignored as non-physical.
    pub radius_of_turn: Option<NumericChannelSummary>,
    /// Source-unit summaries for all numeric columns, including application-defined CAN data.
    ///
    /// VBO does not provide a universal marker for CAN-derived channels, so this vector avoids
    /// guessing their provenance. Select the channels meaningful to the vehicle/application by
    /// their declared names.
    pub numeric_channels: Vec<NumericChannelSummary>,
}

/// High-level telemetry helpers. They never panic on missing channels or malformed samples.
pub trait Telemetry {
    #[must_use]
    fn time_seconds(&self, row: usize) -> Option<f64>;
    #[must_use]
    fn geo_point(&self, row: usize) -> Option<GeoPoint>;
    #[must_use]
    fn satellite_quality(&self, row: usize) -> Option<SatelliteQuality>;
    #[must_use]
    fn analyse(&self) -> SessionMetrics;
}

impl Telemetry for Vbo {
    fn time_seconds(&self, row: usize) -> Option<f64> {
        self.alias_column(&["time"])
            .and_then(|column| self.value(row, column))
            .and_then(parse_time_seconds)
    }

    fn geo_point(&self, row: usize) -> Option<GeoPoint> {
        let latitude = self
            .alias_column(&["lat", "latitude"])
            .and_then(|column| self.value(row, column))?;
        let longitude = self
            .alias_column(&["long", "longitude", "lon"])
            .and_then(|column| self.value(row, column))?;
        self.decode_geo_point(latitude, longitude)
    }

    fn satellite_quality(&self, row: usize) -> Option<SatelliteQuality> {
        let raw = self
            .alias_column(&["sats", "satellites"])
            .and_then(|column| self.value(row, column))?;
        (raw.is_finite() && raw >= 0.0 && raw <= f64::from(u8::MAX) && raw.fract() == 0.0).then(
            || {
                // Bounds and integrality are checked in the predicate above.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let raw = raw as u8;
                SatelliteQuality {
                    satellites: raw & 0b0011_1111,
                    brake_trigger: raw & 64 != 0,
                    dgps: raw & 128 != 0,
                }
            },
        )
    }

    fn analyse(&self) -> SessionMetrics {
        let speed_column = self.alias_column(&["velocity", "speed", "velocitykmh"]);
        let speed_factor = speed_column
            .and_then(|column| self.channels[column].unit.as_deref())
            .and_then(speed_to_kmh_factor);
        let time_column = self.alias_column(&["time"]);
        let latitude_column = self.alias_column(&["lat", "latitude"]);
        let longitude_column = self.alias_column(&["long", "longitude", "lon"]);
        let native_channels = self.native_channel_metrics();
        let mut result = SessionMetrics {
            samples: self.row_count(),
            longitudinal_acceleration: native_channels.longitudinal_acceleration,
            lateral_acceleration: native_channels.lateral_acceleration,
            vertical_acceleration: native_channels.vertical_acceleration,
            yaw_rate: native_channels.yaw_rate,
            radius_of_turn: native_channels.radius_of_turn,
            numeric_channels: self.numeric_channel_summaries(),
            ..SessionMetrics::default()
        };
        let mut previous_time = None;
        let mut previous_speed = None;
        let mut previous_point = None;
        let mut elapsed = 0.0;
        let mut distance = 0.0;
        let mut speed_sum = 0.0;
        let mut speed_count = 0usize;

        for row in 0..self.row_count() {
            let time = time_column
                .and_then(|column| self.value(row, column))
                .and_then(parse_time_seconds);
            if let (Some(last_time), Some(time)) = (previous_time, time) {
                if let Some(delta) = forward_delta(last_time, time) {
                    elapsed += delta;
                } else {
                    result.timing_anomalies += 1;
                }
            }
            if let Some(time) = time {
                previous_time = Some(time);
            }
            if let Some(speed) = speed_column
                .zip(speed_factor)
                .and_then(|(column, factor)| self.value(row, column).map(|speed| speed * factor))
                .filter(|value| value.is_finite() && *value >= 0.0)
            {
                result.max_speed_kmh =
                    Some(result.max_speed_kmh.map_or(speed, |max| max.max(speed)));
                speed_sum += speed;
                speed_count += 1;
                if let (Some((last_speed, last_time)), Some(time)) = (previous_speed, time) {
                    if let Some(delta) = forward_delta(last_time, time) {
                        if delta > 0.0 {
                            let acceleration = (speed - last_speed) / 3.6 / delta;
                            result.peak_acceleration_mps2 = Some(
                                result
                                    .peak_acceleration_mps2
                                    .map_or(acceleration, |peak| peak.max(acceleration)),
                            );
                            result.peak_braking_mps2 = Some(
                                result
                                    .peak_braking_mps2
                                    .map_or(acceleration, |peak| peak.min(acceleration)),
                            );
                        }
                    }
                }
                if let Some(time) = time {
                    previous_speed = Some((speed, time));
                }
            }
            let point = latitude_column
                .and_then(|column| self.value(row, column))
                .zip(longitude_column.and_then(|column| self.value(row, column)))
                .and_then(|(latitude, longitude)| self.decode_geo_point(latitude, longitude));
            if let Some(point) = point {
                if let Some(last) = previous_point {
                    distance += haversine_metres(last, point);
                }
                previous_point = Some(point);
            }
        }
        result.duration_seconds = (elapsed > 0.0).then_some(elapsed);
        result.distance_metres = (self.row_count() > 1).then_some(distance);
        let speed_count = u32::try_from(speed_count).unwrap_or(u32::MAX);
        result.mean_speed_kmh = (speed_count > 0).then_some(speed_sum / f64::from(speed_count));
        result
    }
}

impl Vbo {
    fn alias_column(&self, aliases: &[&str]) -> Option<usize> {
        find_channel(&self.channels, aliases)
    }

    /// Decodes one `lat`/`long` sample pair using this recording's detected
    /// [`CoordinateFormat`].
    fn decode_geo_point(&self, latitude: f64, longitude: f64) -> Option<GeoPoint> {
        match self.coordinate_format {
            CoordinateFormat::PackedDegreesMinutes => Some(GeoPoint {
                latitude_deg: packed_minutes_to_degrees(latitude, CoordinateAxis::Latitude)?,
                longitude_deg: packed_minutes_to_degrees(longitude, CoordinateAxis::Longitude)?,
            }),
            CoordinateFormat::ContinuousMinutes => {
                continuous_minutes_to_degrees(latitude, longitude)
            }
        }
    }

    fn numeric_channel_summaries(&self) -> Vec<NumericChannelSummary> {
        let mut accumulators = vec![ChannelSummaryAccumulator::default(); self.column_count()];
        for row in 0..self.row_count() {
            for (column, accumulator) in accumulators.iter_mut().enumerate() {
                if let Some(value) = self.value(row, column).filter(|value| value.is_finite()) {
                    accumulator.push(value);
                }
            }
        }
        accumulators
            .into_iter()
            .zip(&self.channels)
            .filter_map(|(summary, channel)| {
                summary.finish(channel.name.clone(), channel.unit.clone())
            })
            .collect()
    }

    fn native_channel_metrics(&self) -> NativeChannelMetrics {
        NativeChannelMetrics {
            longitudinal_acceleration: self
                .alias_column(LONGITUDINAL_ACCELERATION_ALIASES)
                .and_then(|column| self.converted_channel(column, acceleration_to_mps2, "m/s²")),
            lateral_acceleration: self
                .alias_column(LATERAL_ACCELERATION_ALIASES)
                .and_then(|column| self.converted_channel(column, acceleration_to_mps2, "m/s²")),
            vertical_acceleration: self
                .alias_column(VERTICAL_ACCELERATION_ALIASES)
                .and_then(|column| self.converted_channel(column, acceleration_to_mps2, "m/s²")),
            yaw_rate: self.alias_column(YAW_RATE_ALIASES).and_then(|column| {
                self.converted_channel(column, yaw_rate_to_degrees_per_second, "deg/s")
            }),
            radius_of_turn: self
                .alias_column(RADIUS_OF_TURN_ALIASES)
                .and_then(|column| {
                    self.converted_channel_with_filter(column, length_to_metres, "m", |value| {
                        value >= 0.0
                    })
                }),
        }
    }

    fn converted_channel(
        &self,
        column: usize,
        factor: fn(&str) -> Option<f64>,
        canonical_unit: &str,
    ) -> Option<NumericChannelSummary> {
        self.converted_channel_with_filter(column, factor, canonical_unit, |_| true)
    }

    fn converted_channel_with_filter(
        &self,
        column: usize,
        factor: fn(&str) -> Option<f64>,
        canonical_unit: &str,
        accept: impl Fn(f64) -> bool,
    ) -> Option<NumericChannelSummary> {
        let source_unit = self.channels.get(column)?.unit.as_deref()?;
        let factor = factor(source_unit)?;
        let mut summary = ChannelSummaryAccumulator::default();
        for row in 0..self.row_count() {
            if let Some(value) = self
                .value(row, column)
                .map(|value| value * factor)
                .filter(|value| value.is_finite() && accept(*value))
            {
                summary.push(value);
            }
        }
        summary.finish(
            self.channels.get(column)?.name.clone(),
            Some(canonical_unit.to_owned()),
        )
    }
}

#[derive(Default)]
struct NativeChannelMetrics {
    longitudinal_acceleration: Option<NumericChannelSummary>,
    lateral_acceleration: Option<NumericChannelSummary>,
    vertical_acceleration: Option<NumericChannelSummary>,
    yaw_rate: Option<NumericChannelSummary>,
    radius_of_turn: Option<NumericChannelSummary>,
}

#[derive(Clone, Default)]
struct ChannelSummaryAccumulator {
    samples: usize,
    minimum: f64,
    maximum: f64,
    sum: f64,
}

impl ChannelSummaryAccumulator {
    fn push(&mut self, value: f64) {
        if self.samples == 0 {
            self.minimum = value;
            self.maximum = value;
        } else {
            self.minimum = self.minimum.min(value);
            self.maximum = self.maximum.max(value);
        }
        self.samples += 1;
        self.sum += value;
    }

    fn finish(self, channel: String, unit: Option<String>) -> Option<NumericChannelSummary> {
        let samples = u32::try_from(self.samples).unwrap_or(u32::MAX);
        (samples > 0).then_some(NumericChannelSummary {
            channel,
            unit,
            samples: self.samples,
            minimum: self.minimum,
            maximum: self.maximum,
            mean: self.sum / f64::from(samples),
        })
    }
}

const LONGITUDINAL_ACCELERATION_ALIASES: &[&str] = &[
    "longacc",
    "longitudinalacceleration",
    "longitudinalaccel",
    "accelx",
    "xaccel",
    "xacceleration",
];
const LATERAL_ACCELERATION_ALIASES: &[&str] = &[
    "latacc",
    "lateralacceleration",
    "lateralaccel",
    "accely",
    "yaccel",
    "yacceleration",
];
const VERTICAL_ACCELERATION_ALIASES: &[&str] = &[
    "vertacc",
    "verticalacc",
    "verticalacceleration",
    "verticalaccel",
    "accelz",
    "zaccel",
    "zacceleration",
];
const YAW_RATE_ALIASES: &[&str] = &["yawrate", "yawangularrate", "angularratez"];
const RADIUS_OF_TURN_ALIASES: &[&str] = &["radiusofturn", "turnradius", "radius"];

/// Converts the VBO `DDMM.MMMM` / `DDDMM.MMMM` representation to decimal degrees.
/// Racelogic documents positive longitude as west; this function returns conventional longitude,
/// so a positive VBO longitude becomes a negative decimal-degree longitude.
#[must_use]
pub fn packed_minutes_to_degrees(value: f64, axis: CoordinateAxis) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let absolute = value.abs();
    let degrees = (absolute / 100.0).floor();
    let minutes = absolute - degrees * 100.0;
    if minutes >= 60.0
        || degrees
            > match axis {
                CoordinateAxis::Latitude => 90.0,
                CoordinateAxis::Longitude => 180.0,
            }
    {
        return None;
    }
    let conventional_sign = match axis {
        CoordinateAxis::Latitude => value.signum(),
        CoordinateAxis::Longitude => -value.signum(),
    };
    Some(conventional_sign * (degrees + minutes / 60.0))
}

fn parse_time_seconds(value: f64) -> Option<f64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let whole = value.trunc();
    let hours = (whole / 10_000.0).floor();
    let minutes = ((whole / 100.0).floor()).rem_euclid(100.0);
    let seconds = whole.rem_euclid(100.0);
    (hours < 24.0 && minutes < 60.0 && seconds < 60.0)
        .then_some(hours * 3600.0 + minutes * 60.0 + seconds + value.fract())
}

fn forward_delta(previous: f64, current: f64) -> Option<f64> {
    let delta = current - previous;
    if delta >= 0.0 {
        Some(delta)
    } else if delta < -43_200.0 {
        Some(delta + 86_400.0)
    } else {
        None
    }
}

fn speed_to_kmh_factor(unit: &str) -> Option<f64> {
    match normalise(unit).as_str() {
        "kmh" | "kph" | "kilometresperhour" | "kilometersperhour" => Some(1.0),
        "knots" | "knot" | "kts" => Some(1.852),
        "mph" | "milesperhour" => Some(1.609_344),
        "ms" | "metrespersecond" | "meterspersecond" => Some(3.6),
        _ => None,
    }
}

fn acceleration_to_mps2(unit: &str) -> Option<f64> {
    match normalise(unit).as_str() {
        "g" | "gee" | "gravity" | "gravities" => Some(9.806_65),
        "ms2" | "mps2" | "metrespersecondsquared" | "meterspersecondsquared" => Some(1.0),
        "fts2" | "fps2" | "feetpersecondsquared" => Some(0.3048),
        _ => None,
    }
}

fn yaw_rate_to_degrees_per_second(unit: &str) -> Option<f64> {
    match normalise(unit).as_str() {
        "degs" | "degsec" | "degreespersecond" | "degreessecond" => Some(1.0),
        "rads" | "radsec" | "radianspersecond" | "radianssecond" => {
            Some(180.0 / std::f64::consts::PI)
        }
        _ => None,
    }
}

fn length_to_metres(unit: &str) -> Option<f64> {
    match normalise(unit).as_str() {
        "m" | "metre" | "metres" | "meter" | "meters" => Some(1.0),
        "km" | "kilometre" | "kilometres" | "kilometer" | "kilometers" => Some(1_000.0),
        "ft" | "foot" | "feet" => Some(0.3048),
        "mi" | "mile" | "miles" => Some(1_609.344),
        _ => None,
    }
}

fn haversine_metres(a: GeoPoint, b: GeoPoint) -> f64 {
    let radians = std::f64::consts::PI / 180.0;
    let d_lat = (b.latitude_deg - a.latitude_deg) * radians;
    let d_lon = (b.longitude_deg - a.longitude_deg) * radians;
    let h = (d_lat / 2.0).sin().powi(2)
        + a.latitude_deg.to_radians().cos()
            * b.latitude_deg.to_radians().cos()
            * (d_lon / 2.0).sin().powi(2);
    6_371_008.8 * 2.0 * h.sqrt().atan2((1.0 - h).sqrt())
}
