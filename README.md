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

## Quality gates

```bash
./scripts/setup-hooks.sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Releases are published only by CI after pushing an annotated version tag such as `v0.1.0`. Configure the repository’s `CARGO_REGISTRY_TOKEN` GitHub Actions secret; do not commit a local crates.io key.
