# racelogic-vbo

`racelogic-vbo` is a safety-oriented, high-throughput Rust library for legacy Racelogic VBOX `.vbo` telemetry logs. It parses the sectioned ASCII format into a compact row-major table, exposes zero-copy sample views, and calculates useful session metrics without requiring a database or runtime.

> Status: pre-1.0. The parser targets the legacy VBO family, not VBOX 3i's VBB or VBOX Sport's VBS formats.

## Design goals

- **Fast hot path:** a single input read, byte-oriented newline discovery, no per-field allocation, and contiguous `f64` storage.
- **Fault containment:** strict parsing is transactional; recovery mode drops only malformed records and returns line-level diagnostics. Bounds prevent oversized lines and unbounded rows.
- **Telemetry that respects the format:** packed coordinates, UTC time-of-day rollover, VBO satellite flags, distance, speed, and longitudinal acceleration are handled explicitly.
- **Stable operations:** no `unsafe`, CI on Linux/macOS/Windows, dependency auditing, reproducible release gates, and committed local hooks.

## Quick start

```rust
use racelogic_vbo::{parse_path, Telemetry};

let session = parse_path("run.vbo")?;
let metrics = session.analyse();
println!("distance: {:?} m", metrics.distance_metres);
# Ok::<(), racelogic_vbo::ParseError>(())
```

For a detailed compatibility note, source links, and the exact coordinate convention, see [the VBO format reference](docs/VBO_FORMAT.md). Planned scope is tracked in [PLAN.md](PLAN.md).

## Large recordings

`parse_path` and `parse_reader` retain the complete table for random access and analysis. For
large or unbounded logs, use `Parser::parse_bufread` with a `BufRead` source instead: it reuses
one row buffer and delivers samples synchronously to your callback, retaining only header data
and bounded recovery diagnostics. This is the recommended ingestion path for multi-GB recordings
or direct database/Parquet export pipelines.

```rust
use std::{fs::File, io::BufReader};
use racelogic_vbo::Parser;

let mut speed_total = 0.0;
let report = Parser::default().parse_bufread(BufReader::new(File::open("run.vbo")?), |row| {
    speed_total += row.value(4).unwrap_or_default();
})?;
println!("processed {} samples", report.row_count());
# Ok::<(), racelogic_vbo::ParseError>(())
```

## Quality gates

```bash
./scripts/setup-hooks.sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo bench
```

## Benchmarks

The reproducible in-memory benchmarks exercise strict parsing and telemetry analysis over a
100,000-sample, 10 Hz VBO session (including time, position, speed, and satellite channels):

```bash
cargo bench
```

To compare Rust's standard numeric conversion with the opt-in optimized parser, run:

```bash
cargo bench --features fast-float
```

## Analysis and export

The analysis API provides gate-based lap/sector timing plus interpolation, uniform resampling,
smoothing, and alignment for finite time series. `Telemetry::analyse` also summarises declared
accelerometer, yaw-rate, turn-radius, and application/CAN-like numeric channels without guessing
units or proprietary signal meanings.

CSV, GPX, and serde support are enabled by default. Apache Parquet is opt-in because of its
larger dependency footprint:

```bash
cargo add racelogic-vbo --features parquet
```

All writers accept a caller-owned `Write` sink (`Vbo::write_csv`, `write_gpx`,
`write_parquet`) so applications can choose their own atomic-file strategy.

## Fuzzing

The repository includes a bounded recovery-parser fuzz target and a scheduled GitHub Actions
smoke run. Locally, install `cargo-fuzz` and use `cd fuzz && cargo fuzz run parser`.

Releases are published only by CI after pushing an annotated version tag such as `v0.1.0`. Configure the repository’s `CARGO_REGISTRY_TOKEN` GitHub Actions secret; do not commit a local crates.io key.
