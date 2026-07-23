# Changelog

All notable changes follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-07-23

### Fixed

- `Telemetry::geo_point` and `Telemetry::analyse` now auto-detect a recording's `lat`/`long` coordinate convention instead of assuming the documented packed `DDMM.MMMM` format unconditionally. Some hardware (observed on a Video VBOX HD2 dashcam unit) logs continuous minutes instead, which previously made every coordinate on those recordings silently fail to decode. Detection is per-recording and non-breaking: files that already decode correctly under the packed convention are unaffected. See `docs/VBO_FORMAT.md` and the new `CoordinateFormat` type.

## [0.1.0] - 2026-07-22

### Added

- Strict and recovery-mode legacy VBO parser.
- Bounded-memory `BufRead` streaming parser for large recordings.
- Core GPS/time/satellite telemetry metrics.
- Unit-normalised inertial, yaw-rate, turn-radius, and generic numeric/CAN summaries.
- Gate-based lap and sector timing, interpolation, resampling, smoothing, and alignment.
- CSV, GPX, serde, and optional Apache Parquet exports.
- Optional `fast-float` parser backend with benchmark and semantic parity tests.
- Corpus-seeded parser fuzzing with scheduled CI coverage.
- Reproducible parser and telemetry benchmarks.
- GitHub CI, pre-commit quality gate, and tag-driven release workflow.
- Dependency policy, Dependabot, security policy, code ownership, and contribution templates.
