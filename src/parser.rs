use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use memchr::memchr_iter;

use crate::{Channel, Header, ParseError, ParseIssue, ParseIssueKind, ParseReport, Vbo};

/// Resource and compatibility controls for [`Parser`].
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// Reject a bad data row rather than omitting it. Enabled by default.
    pub strict: bool,
    /// Hard cap for a physical input line, protecting services from pathological inputs.
    pub max_line_bytes: usize,
    /// Hard cap for accepted data rows.
    pub max_rows: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            strict: true,
            max_line_bytes: 1024 * 1024,
            max_rows: 20_000_000,
        }
    }
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
    pub fn parse_reader<R: Read>(&self, mut reader: R) -> Result<Vbo, ParseError> {
        let mut input = Vec::new();
        reader.read_to_end(&mut input)?;
        self.parse_bytes(&input, false).map(|report| report.vbo)
    }

    /// Parses while omitting malformed data records and collecting their diagnostics.
    /// Header-structure errors remain fatal because they make column alignment ambiguous.
    pub fn parse_reader_recovering<R: Read>(
        &self,
        mut reader: R,
    ) -> Result<ParseReport, ParseError> {
        let mut input = Vec::new();
        reader.read_to_end(&mut input)?;
        self.parse_bytes(&input, true)
    }

    pub fn parse_str(&self, input: &str) -> Result<Vbo, ParseError> {
        self.parse_bytes(input.as_bytes(), false)
            .map(|report| report.vbo)
    }

    #[allow(clippy::too_many_lines)] // The state machine is deliberately kept in one audit-friendly place.
    fn parse_bytes(&self, input: &[u8], force_recovery: bool) -> Result<ParseReport, ParseError> {
        let mut header = Header::default();
        let mut channels = Vec::new();
        let mut units = Vec::new();
        let mut values = Vec::new();
        let mut issues = Vec::new();
        let mut section = Section::Preamble;
        let mut data_seen = false;
        let mut rows = 0usize;
        let recover = force_recovery || !self.options.strict;
        let mut start = 0usize;

        for (line, newline) in
            (1usize..).zip(memchr_iter(b'\n', input).chain(std::iter::once(input.len())))
        {
            let mut raw = &input[start..newline];
            start = newline.saturating_add(1);
            if raw.last() == Some(&b'\r') {
                raw = &raw[..raw.len() - 1];
            }
            if raw.len() > self.options.max_line_bytes {
                if recover && matches!(section, Section::Data) {
                    issues.push(ParseIssue {
                        line,
                        kind: ParseIssueKind::LineTooLong {
                            limit: self.options.max_line_bytes,
                        },
                    });
                    continue;
                }
                return Err(ParseError::LineTooLong {
                    line,
                    limit: self.options.max_line_bytes,
                });
            }
            let raw = trim_ascii(raw);
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
                    match parse_row_into(text, channels.len(), line, &mut values) {
                        Ok(()) => {
                            rows += 1;
                        }
                        Err(error) if recover => issues.push(issue_from(error)),
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
        for (channel, unit) in channels.iter_mut().zip(units) {
            channel.unit = Some(unit);
        }
        Ok(ParseReport {
            vbo: Vbo {
                header,
                channels,
                values,
                rows,
            },
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
        let value = field.parse::<f64>().map_err(|_| ParseError::InvalidNumber {
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

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}
