# Product plan

The checked items are implemented and covered by automated tests. This plan intentionally distinguishes the durable 0.x core from compatibility work that needs representative hardware captures.

## Foundation

- [x] Initialise a publishable Rust library crate and Git repository
- [x] Define a documented legacy VBO compatibility contract from primary Racelogic sources
- [x] Parse sectioned headers and space/tab-delimited data rows
- [x] Preserve unknown header metadata and attach optional channel units
- [x] Store numeric samples as contiguous row-major `f64` data with zero-copy row views
- [x] Enforce resource limits for rows and line length
- [x] Provide typed, line- and column-aware errors
- [x] Offer recovery mode that excludes malformed rows and returns diagnostics

## Telemetry

- [x] Decode VBO packed latitude/longitude with documented longitude convention
- [x] Decode satellite, DGPS, and brake-trigger bits
- [x] Calculate duration (including UTC midnight rollover), Haversine distance, speed extrema/mean, and derived longitudinal acceleration
- [x] Make metrics unit-aware from `[channel units]` and support km/h, knots, mph, and m/s
- [ ] Native accelerometer, yaw, radius-of-turn, and CAN channel analysis
- [ ] Lap/sector detection using configurable start/finish gates
- [ ] Resampling, smoothing, alignment, and channel interpolation
- [ ] GPX/CSV/Parquet export and serde integration

## Performance and resilience

- [x] Byte-oriented line scanning and allocation-free numeric-field tokenisation
- [x] Fuzz/property tests for coordinate and parser boundary conditions
- [x] Reproducible parser/telemetry benchmark suite (real-world corpus thresholds remain to be added)
- [x] Bounded-memory streaming parser with synchronous row callback and diagnostic limits
- [ ] SIMD/fast-float parsing benchmark and optional optimized parser backend
- [ ] Corpus-based fuzz target and continuous fuzzing
- [ ] Differential tests against VBOX Tools exports and vendor hardware captures

## Delivery and governance

- [x] Unit, integration, and property tests
- [x] Pre-commit hook and bootstrap script
- [x] GitHub CI: formatting, linting, tests, docs, audit, and OS/Rust matrix
- [x] Tag-triggered publishing workflow with version/tag verification and GitHub release
- [ ] Add `CARGO_REGISTRY_TOKEN` as a GitHub Actions repository secret
- [x] Set the canonical GitHub repository URL in `Cargo.toml`
- [x] Add licence, changelog policy, SECURITY.md, CODEOWNERS, and issue/PR templates
- [ ] Add signed tags / trusted publishing and publish the first release
