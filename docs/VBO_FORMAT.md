# Racelogic VBOX `.vbo` format reference

This document defines the compatibility boundary for `racelogic-vbo` 0.x. It is based on Racelogic’s public support article and the VBOX II user guide, both linked below. The VBO family is a legacy Racelogic logged-data format; current product families also include VBB and VBS, which are outside this crate’s first compatibility target.

## Wire format

A VBO is UTF-8-compatible, ASCII, space-delimited text. It contains a textual preamble and named bracket sections, followed by a row-oriented `[data]` body. Whitespace is treated flexibly (spaces, tabs, CRLF/LF). The format is not CSV and must not be parsed as comma-separated data.

```text
File created on 31/07/2006 at 09:55:20
[header]
… human-readable channel labels …
[channel units]
… optional units …
[comments]
… device and logging metadata …
[column names]
sats time lat long velocity heading height vertvel
[data]
137 162235.40 +03119.09973 +00058.49277 000.140 321.85 +00152.58 +000.000
```

`[column names]` establishes the ordered schema for every record in `[data]`. A conforming reader must associate each token strictly by position and reject (or deliberately recover from) rows whose field count differs from that schema. Header content is device and firmware dependent; unknown lines should be preserved, not assumed to be errors.

## Standard channels

| Channel | Encoding / unit | Notes |
| --- | --- | --- |
| `sats` | decimal bit-packed count | low 6 bits: satellite count; bit 6 (`64`): brake trigger; bit 7 (`128`): DGPS active |
| `time` | `HHMMSS.SS` in examples | UTC time since midnight; support documentation also describes `HH:MM:SS.SS` |
| `lat` | `DDMM.MMMMM` | positive is North |
| `long` | `DDDMM.MMMMM` | Racelogic documents positive as West |
| `velocity` | typically km/h, sometimes knots | read unit metadata before interpreting values |
| `heading` | degrees relative to North | |
| `height` | metres, WGS84 reference | |
| `vertvel` | typically km/h or m/s | positive is uphill |
| trigger event | device clock counts | public support documentation says up to 11520 |
| input modules | often scientific notation | `+1.23456E+02` is valid numeric data |

The VBOX II manual additionally describes trigger event time as the interval from the trigger to the preceding GPS sample; different hardware/firmware documentation should take priority when a deployment depends on event timing.

## Coordinate conversion

For a packed absolute value `DDDMM.MMMMM`, calculate `degrees + minutes / 60`. Apply the sign after conversion. Racelogic’s legacy convention is unusual for longitude: **positive VBO longitude means west**. `packed_minutes_to_degrees(..., Longitude)` returns conventional geographic longitude, so it negates a positive VBO longitude. This is intentionally documented and tested to prevent silent east/west mirroring.

## Parser policy

- The default parser requires `[column names]` before `[data]`, validates every numeric token, and is transactional.
- Recovery mode never retains a partial row. It reports the original line and reason then resumes at the next row.
- Input line and row limits defend long-running services against corrupt or hostile recordings.
- Only numeric conversion and recognized section boundaries are semantic. Unknown header metadata is preserved verbatim by section.
- Speed metrics are emitted in km/h only when `[channel units]` declares a recognized velocity unit (`km/h`, knots, mph, or m/s). The parser deliberately does not silently label a unitless `velocity` column as km/h.

## Primary sources

- [Racelogic Support Centre: `.VBO Files`](https://en.racelogic.support/knowledge-bases/general-kb/vbo-files/) — section layout, standard example, satellite flags, coordinate encoding, height, vertical velocity, and input-channel notation.
- [Racelogic VBOX II User Guide (PDF)](https://www.racelogic.co.uk/_downloads/vbox/Manuals/Data_Loggers/RLVB2DCF_Manual.pdf) — VBO example, 20 Hz sample context, and trigger event timing.
- [Racelogic Support Centre: File Formats](https://en.racelogic.support/knowledge-bases/general-kb/file-formats/) — current distinction between VBO, VBB, and VBS product file families.

Source snapshots were consulted on 2026-07-22. Racelogic hardware differs by generation; add real, anonymised fixture files and a compatibility test before broadening this contract.
