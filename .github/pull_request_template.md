## Summary

<!-- State the problem and the change in a few sentences. Link related issues. -->

## Validation

- [ ] I added or updated focused tests for the changed behaviour.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test --all-features` passes.
- [ ] I ran relevant benchmarks or documented why performance is unaffected.

## Parser and telemetry changes

- [ ] Not applicable.
- [ ] Inputs are bounded and malformed input returns diagnostics/errors rather than panicking.
- [ ] The change preserves strict/recovery semantics and documents any compatibility impact.
- [ ] Fixtures are synthetic or redacted and contain no sensitive location data.

## Release impact

- [ ] No user-visible change.
- [ ] Documentation or CHANGELOG updated as appropriate.
- [ ] SemVer impact considered (patch / minor / major).

## Checklist

- [ ] I have read the project's contributing and security expectations.
- [ ] I kept the change focused and did not commit credentials or generated build artifacts.
