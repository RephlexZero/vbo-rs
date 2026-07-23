use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

use memchr::memchr;

use crate::{
    Channel, Header, ParseError, ParseIssue, ParseIssueKind, ParseReport, StreamReport,
    StreamSample, Vbo,
};

/// Resource and compatibility controls for [`Parser`].
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// Reject a bad data row rather than omitting it. Enabled by default.
    pub strict: bool,
    /// Hard cap for a physical input line, protecting services from pathological inputs.
    pub max_line_bytes: usize,
    /// Hard cap for accepted data rows.
    pub max_rows: usize,
    /// Hard cap for retained recovery diagnostics.
    pub max_issues: usize,
    /// Numeric conversion backend used for data fields.
    pub float_parser: FloatParser,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            strict: true,
            max_line_bytes: 1024 * 1024,
            max_rows: 20_000_000,
            max_issues: 10_000,
            float_parser: FloatParser::Standard,
        }
    }
}

/// Numeric conversion backend used by [`Parser`].
///
/// [`Self::Fast`] is available only with the `fast-float` Cargo feature and is intended for
/// high-throughput ASCII telemetry ingestion. The parser preserves the same VBO error shape as
/// the standard backend.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FloatParser {
    /// Rust's standard `f64` parser; the default compatibility backend.
    #[default]
    Standard,
    /// `lexical-core`'s optimized float parser.
    #[cfg(feature = "fast-float")]
    Fast,
}

/// Parser for legacy ASCII Racelogic VBOX `.vbo` recordings.
#[derive(Clone, Debug, Default)]
pub struct Parser {
    options: ParseOptions,
}

impl Parser {
    #[must_use]
    pub fn new(options: ParseOptions) -> Self {
        Self { options }
    }

    #[must_use]
    pub fn options(&self) -> &ParseOptions {
        &self.options
    }

    /// Parses a complete VBO input. Strict mode is transactional: no partial VBO is returned.
    ///
    /// For unbounded recordings, prefer [`Self::parse_bufread`], which does not retain samples.
    pub fn parse_reader<R: Read>(&self, reader: R) -> Result<Vbo, ParseError> {
        let mut values = Vec::new();
        let report = self.parse_bufread(BufReader::new(reader), |sample| {
            values.extend_from_slice(sample.values());
        })?;
        Ok(Vbo::new(
            report.header,
            report.channels,
            values,
            report.rows,
        ))
    }

    /// Parses while omitting malformed data records and collecting their diagnostics.
    /// Header-structure errors remain fatal because they make column alignment ambiguous.
    pub fn parse_reader_recovering<R: Read>(&self, reader: R) -> Result<ParseReport, ParseError> {
        let mut values = Vec::new();
        let report = self.parse_bufread_recovering(BufReader::new(reader), |sample| {
            values.extend_from_slice(sample.values());
        })?;
        Ok(ParseReport {
            vbo: Vbo::new(report.header, report.channels, values, report.rows),
            issues: report.issues,
        })
    }

    pub fn parse_str(&self, input: &str) -> Result<Vbo, ParseError> {
        self.parse_reader(std::io::Cursor::new(input.as_bytes()))
    }

    /// Streams accepted VBO data rows from a buffered reader without retaining the table.
    ///
    /// The callback is called synchronously for each valid row. [`StreamSample`] borrows a
    /// reusable row buffer, so copy values inside the callback only when they must outlive it.
    /// Strict mode stops at the first malformed data row.
    pub fn parse_bufread<R, F>(&self, reader: R, visitor: F) -> Result<StreamReport, ParseError>
    where
        R: BufRead,
        F: FnMut(StreamSample<'_>),
    {
        self.parse_bufread_inner(reader, false, visitor)
    }

    /// Streams valid rows and records malformed data-row diagnostics instead of stopping.
    ///
    /// Header-structure and UTF-8 errors remain fatal because a reliable stream cannot be
    /// formed from them.
    pub fn parse_bufread_recovering<R, F>(
        &self,
        reader: R,
        visitor: F,
    ) -> Result<StreamReport, ParseError>
    where
        R: BufRead,
        F: FnMut(StreamSample<'_>),
    {
        self.parse_bufread_inner(reader, true, visitor)
    }

    #[allow(clippy::too_many_lines)] // The state machine is deliberately kept in one audit-friendly place.
    fn parse_bufread_inner<R, F>(
        &self,
        mut reader: R,
        force_recovery: bool,
        mut visitor: F,
    ) -> Result<StreamReport, ParseError>
    where
        R: BufRead,
        F: FnMut(StreamSample<'_>),
    {
        let mut header = Header::default();
        let mut channels = Vec::new();
        let mut units = Vec::new();
        let mut issues = Vec::new();
        let mut section = Section::Preamble;
        let mut data_seen = false;
        let mut rows = 0usize;
        let recover = force_recovery || !self.options.strict;
        let mut line = 0usize;
        let mut raw = Vec::with_capacity(self.options.max_line_bytes.saturating_add(1));
        let mut row_values = Vec::new();

        while let Some(physical_line_too_long) =
            read_bounded_line(&mut reader, &mut raw, self.options.max_line_bytes)?
        {
            line += 1;
            if raw.last() == Some(&b'\r') {
                raw.pop();
            }
            if physical_line_too_long || raw.len() > self.options.max_line_bytes {
                if recover && matches!(section, Section::Data) {
                    push_issue(
                        &mut issues,
                        ParseIssue {
                            line,
                            kind: ParseIssueKind::LineTooLong {
                                limit: self.options.max_line_bytes,
                            },
                        },
                        self.options.max_issues,
                        line,
                    )?;
                    continue;
                }
                return Err(ParseError::LineTooLong {
                    line,
                    limit: self.options.max_line_bytes,
                });
            }
            let raw = trim_ascii(&raw);
            if raw.is_empty() {
                continue;
            }
            let text = std::str::from_utf8(raw).map_err(|_| ParseError::InvalidUtf8 { line })?;
            if let Some(next) = Section::from_marker(text) {
                if matches!(next, Section::Data) && channels.is_empty() {
                    return Err(ParseError::MissingColumnNames { line });
                }
                if matches!(next, Section::Data) {
                    data_seen = true;
                    apply_units(&mut channels, &units);
                }
                section = next;
                continue;
            }
            match section {
                Section::Preamble => {
                    if header.created.is_none()
                        && text.to_ascii_lowercase().starts_with("file created")
                    {
                        header.created = Some(text.to_owned());
                    } else {
                        header
                            .sections
                            .entry("preamble".into())
                            .or_default()
                            .push(text.to_owned());
                    }
                }
                Section::ColumnNames => {
                    channels.extend(text.split_ascii_whitespace().map(|name| Channel {
                        name: name.to_owned(),
                        unit: None,
                    }));
                }
                Section::ChannelUnits => {
                    units.extend(text.split_ascii_whitespace().map(str::to_owned));
                }
                Section::Data => {
                    if rows >= self.options.max_rows {
                        return Err(ParseError::RowLimit {
                            line,
                            limit: self.options.max_rows,
                        });
                    }
                    row_values.clear();
                    match parse_row_into(
                        text,
                        channels.len(),
                        line,
                        &mut row_values,
                        self.options.float_parser,
                    ) {
                        Ok(()) => {
                            visitor(StreamSample::new(
                                rows,
                                line,
                                &row_values,
                                &header,
                                &channels,
                            ));
                            rows += 1;
                        }
                        Err(error) if recover => push_issue(
                            &mut issues,
                            issue_from(error),
                            self.options.max_issues,
                            line,
                        )?,
                        Err(error) => return Err(error),
                    }
                }
                section => header
                    .sections
                    .entry(section.name().to_owned())
                    .or_default()
                    .push(text.to_owned()),
            }
        }
        if !data_seen {
            return Err(ParseError::MissingData);
        }
        apply_units(&mut channels, &units);
        Ok(StreamReport {
            header,
            channels,
            rows,
            issues,
        })
    }
}

/// Parses a VBO file from disk with default strict options.
pub fn parse_path(path: impl AsRef<Path>) -> Result<Vbo, ParseError> {
    Parser::default().parse_reader(BufReader::new(File::open(path)?))
}

#[derive(Clone, Copy, Debug)]
enum Section {
    Preamble,
    Header,
    ChannelUnits,
    Comments,
    ColumnNames,
    Data,
    Other,
}

impl Section {
    fn from_marker(text: &str) -> Option<Self> {
        let marker = text
            .strip_prefix('[')?
            .strip_suffix(']')?
            .trim()
            .to_ascii_lowercase();
        Some(match marker.as_str() {
            "header" => Self::Header,
            "channel units" => Self::ChannelUnits,
            "comments" => Self::Comments,
            "column names" => Self::ColumnNames,
            "data" => Self::Data,
            _ => Self::Other,
        })
    }
    const fn name(self) -> &'static str {
        match self {
            Self::Preamble => "preamble",
            Self::Header => "header",
            Self::ChannelUnits => "channel units",
            Self::Comments => "comments",
            Self::ColumnNames => "column names",
            Self::Data => "data",
            Self::Other => "other",
        }
    }
}

fn parse_row_into(
    text: &str,
    expected: usize,
    line: usize,
    output: &mut Vec<f64>,
    float_parser: FloatParser,
) -> Result<(), ParseError> {
    let found = text.split_ascii_whitespace().count();
    if found != expected {
        return Err(ParseError::WrongColumnCount {
            line,
            expected,
            found,
        });
    }
    let row_start = output.len();
    output.reserve(expected);
    for (index, field) in text.split_ascii_whitespace().enumerate() {
        let value = parse_float(field, float_parser).map_err(|()| ParseError::InvalidNumber {
            line,
            column: index + 1,
            value: field.to_owned(),
        });
        match value {
            Ok(value) => output.push(value),
            Err(error) => {
                output.truncate(row_start);
                return Err(error);
            }
        }
    }
    Ok(())
}

fn parse_float(field: &str, float_parser: FloatParser) -> Result<f64, ()> {
    match float_parser {
        FloatParser::Standard => field.parse::<f64>().map_err(|_| ()),
        #[cfg(feature = "fast-float")]
        FloatParser::Fast => lexical_core::parse::<f64>(field.as_bytes()).map_err(|_| ()),
    }
}

fn issue_from(error: ParseError) -> ParseIssue {
    match error {
        ParseError::WrongColumnCount {
            line,
            expected,
            found,
        } => ParseIssue {
            line,
            kind: ParseIssueKind::WrongColumnCount { expected, found },
        },
        ParseError::InvalidNumber {
            line,
            column,
            value,
        } => ParseIssue {
            line,
            kind: ParseIssueKind::InvalidNumber { column, value },
        },
        ParseError::LineTooLong { line, limit } => ParseIssue {
            line,
            kind: ParseIssueKind::LineTooLong { limit },
        },
        _ => unreachable!("only record-local parser errors are recoverable"),
    }
}

fn push_issue(
    issues: &mut Vec<ParseIssue>,
    issue: ParseIssue,
    limit: usize,
    line: usize,
) -> Result<(), ParseError> {
    if issues.len() >= limit {
        return Err(ParseError::IssueLimit { line, limit });
    }
    issues.push(issue);
    Ok(())
}

fn apply_units(channels: &mut [Channel], units: &[String]) {
    for (channel, unit) in channels.iter_mut().zip(units) {
        channel.unit = Some(unit.clone());
    }
}

/// Reads one physical line while retaining at most `limit + 1` bytes.
///
/// The extra byte distinguishes a line at the limit followed by CRLF from a genuinely oversized
/// line. The rest of an oversized line is consumed directly from `BufRead` without allocating.
fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<Option<bool>> {
    output.clear();
    let storage_limit = limit.saturating_add(1);
    let mut saw_line = false;
    let mut too_long = false;

    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(saw_line.then_some(too_long));
        }

        let newline = memchr(b'\n', buffer);
        let bytes_before_newline = newline.unwrap_or(buffer.len());
        if bytes_before_newline > 0 {
            saw_line = true;
            let available = storage_limit.saturating_sub(output.len());
            let retained = bytes_before_newline.min(available);
            output.extend_from_slice(&buffer[..retained]);
            too_long |= retained < bytes_before_newline;
        }

        let consumed = newline.map_or(bytes_before_newline, |index| index + 1);
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some(too_long));
        }
    }
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}
