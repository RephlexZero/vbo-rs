---
name: Bug report
about: Report a reproducible defect in parsing, telemetry, or public APIs
title: "bug: "
labels: bug
assignees: ""
---

## Summary

<!-- What happened, and what did you expect instead? -->

## Reproduction

<!-- Include a minimal Rust snippet, command, and synthetic/redacted VBO input. -->

```text
# Command and/or fixture
```

## Actual result

<!-- Include the error, panic backtrace, or incorrect values. -->

## Expected result

<!-- Include expected samples, diagnostics, or telemetry values. -->

## Environment

- crate version / commit:
- Rust version (`rustc -Vv`):
- operating system:
- feature flags:

## Input characteristics

- file size:
- number of samples (if known):
- strict or recovery parser mode:
- does the input contain non-default channels or malformed records?

## Safety and privacy

- [ ] I removed precise coordinates, timestamps, identifiers, and credentials.
- [ ] I can share a minimal synthetic or redacted fixture that reproduces this.

## Additional context

<!-- Benchmarks, profiler output, or proposed fixes are welcome. -->
