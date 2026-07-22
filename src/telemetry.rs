use crate::{types::normalise, Vbo};

/// Axis semantics for Racelogic's packed-minutes coordinate encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateAxis {
    Latitude,
    Longitude,
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
        Some(GeoPoint {
            latitude_deg: packed_minutes_to_degrees(latitude, CoordinateAxis::Latitude)?,
            longitude_deg: packed_minutes_to_degrees(longitude, CoordinateAxis::Longitude)?,
        })
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
        let mut result = SessionMetrics {
            samples: self.row_count(),
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
                .and_then(|(latitude, longitude)| {
                    Some(GeoPoint {
                        latitude_deg: packed_minutes_to_degrees(
                            latitude,
                            CoordinateAxis::Latitude,
                        )?,
                        longitude_deg: packed_minutes_to_degrees(
                            longitude,
                            CoordinateAxis::Longitude,
                        )?,
                    })
                });
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
        self.channels.iter().position(|channel| {
            aliases
                .iter()
                .any(|alias| normalise(&channel.name) == *alias)
        })
    }
}

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
