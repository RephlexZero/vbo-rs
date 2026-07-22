use std::{collections::BTreeMap, fmt};

use thiserror::Error;

/// A named data channel declared by `[column names]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Channel {
    pub name: String,
    pub unit: Option<String>,
}

/// The non-tabular contents of a VBO header, preserved in source order by section.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Header {
    pub created: Option<String>,
    pub sections: BTreeMap<String, Vec<String>>,
}

/// An owned, cache-friendly VBO table. Values are stored row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Vbo {
    pub header: Header,
    pub channels: Vec<Channel>,
    pub(crate) values: Vec<f64>,
    pub(crate) rows: usize,
}

impl Vbo {
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows
    }

    #[must_use]
    pub fn column_count(&self) -> usize {
        self.channels.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// Looks up a column case-insensitively, ignoring whitespace, `_`, and `-`.
    #[must_use]
    pub fn channel_index(&self, name: &str) -> Option<usize> {
        let target = normalise(name);
        self.channels
            .iter()
            .position(|channel| normalise(&channel.name) == target)
    }

    #[must_use]
    pub fn value(&self, row: usize, column: usize) -> Option<f64> {
        (row < self.rows && column < self.channels.len())
            .then(|| self.values[row * self.channels.len() + column])
    }

    #[must_use]
    pub fn sample(&self, row: usize) -> Option<SampleRef<'_>> {
        (row < self.rows).then_some(SampleRef { vbo: self, row })
    }

    #[must_use]
    pub fn samples(&self) -> impl ExactSizeIterator<Item = SampleRef<'_>> + '_ {
        (0..self.rows).map(|row| SampleRef { vbo: self, row })
    }
}

/// A zero-copy view of one logged sample.
#[derive(Clone, Copy)]
pub struct SampleRef<'a> {
    vbo: &'a Vbo,
    row: usize,
}

impl SampleRef<'_> {
    #[must_use]
    pub fn row_index(self) -> usize {
        self.row
    }

    #[must_use]
    pub fn get(self, column: usize) -> Option<f64> {
        self.vbo.value(self.row, column)
    }
}

impl fmt::Debug for SampleRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SampleRef")
            .field("row", &self.row)
            .finish_non_exhaustive()
    }
}

/// A non-fatal defect encountered in recovery mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseIssue {
    pub line: usize,
    pub kind: ParseIssueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseIssueKind {
    InvalidNumber { column: usize, value: String },
    WrongColumnCount { expected: usize, found: usize },
    LineTooLong { limit: usize },
}

/// The result of recovery parsing. Malformed data records are omitted, never partially retained.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseReport {
    pub vbo: Vbo,
    pub issues: Vec<ParseIssue>,
}

/// A contextual failure while reading or decoding VBO input.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("I/O error while reading VBO: {0}")]
    Io(#[from] std::io::Error),
    #[error("input is not valid UTF-8 at line {line}")]
    InvalidUtf8 { line: usize },
    #[error("line {line} exceeds configured {limit}-byte limit")]
    LineTooLong { line: usize, limit: usize },
    #[error("line {line}: [data] appears before [column names]")]
    MissingColumnNames { line: usize },
    #[error("missing required [data] section")]
    MissingData,
    #[error("line {line}: expected {expected} columns, found {found}")]
    WrongColumnCount {
        line: usize,
        expected: usize,
        found: usize,
    },
    #[error("line {line}, column {column}: invalid number `{value}`")]
    InvalidNumber {
        line: usize,
        column: usize,
        value: String,
    },
    #[error("configured row limit ({limit}) exceeded at line {line}")]
    RowLimit { line: usize, limit: usize },
}

pub(crate) fn normalise(value: &str) -> String {
    value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect()
}
