//! Lossless tabular and geographic export helpers.
//!
//! The functions write to a caller-owned [`std::io::Write`] target, allowing atomic file
//! replacement to be handled by the application. They never construct paths or follow links.

use std::io::{self, Write};

use thiserror::Error;

use crate::{Telemetry, Vbo};

#[cfg(feature = "parquet")]
const PARQUET_ROW_GROUP_SIZE: usize = 65_536;

/// A failure while encoding an export. Partial output is possible if the supplied writer fails.
#[derive(Debug, Error)]
pub enum ExportError {
    /// The destination rejected output.
    #[error("I/O error while writing export: {0}")]
    Io(#[from] io::Error),
    /// CSV encoding failed.
    #[cfg(feature = "csv")]
    #[error("CSV encoding error: {0}")]
    Csv(#[from] csv::Error),
    /// Apache Parquet encoding failed.
    #[cfg(feature = "parquet")]
    #[error("Parquet encoding error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    /// The requested representation requires at least one source column.
    #[error("cannot export a VBO with no channels")]
    NoChannels,
    /// The recording contains no valid geographic samples for a GPX track.
    #[error("cannot export GPX because the recording has no valid latitude/longitude samples")]
    NoTrackPoints,
}

impl Vbo {
    /// Writes the source table as RFC 4180-style CSV with channel names as its first record.
    ///
    /// Channel names are emitted verbatim; the CSV encoder quotes commas, quotes, and newlines.
    #[cfg(feature = "csv")]
    pub fn write_csv<W: Write>(&self, writer: W) -> Result<(), ExportError> {
        ensure_channels(self)?;
        let mut csv = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(writer);
        csv.write_record(self.channels.iter().map(|channel| channel.name.as_str()))?;
        let width = self.channels.len();
        for values in self.values.chunks_exact(width) {
            csv.serialize(values)?;
        }
        csv.flush()?;
        Ok(())
    }

    /// Writes a GPX 1.1 track containing every valid VBO latitude/longitude sample.
    ///
    /// VBO times have no calendar date, so they are preserved as `vbo:time-seconds` extensions
    /// rather than invalid GPX `<time>` values. Rows with invalid coordinates are omitted.
    #[cfg(feature = "gpx")]
    pub fn write_gpx<W: Write>(&self, mut writer: W) -> Result<(), ExportError> {
        if !self
            .samples()
            .any(|sample| self.geo_point(sample.row_index()).is_some())
        {
            return Err(ExportError::NoTrackPoints);
        }

        write!(
            writer,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"racelogic-vbo\" xmlns=\"http://www.topografix.com/GPX/1/1\" xmlns:vbo=\"https://github.com/RephlexZero/vbo-rs\">\n<trk>"
        )?;
        if let Some(created) = &self.header.created {
            write!(writer, "<name>")?;
            write_xml_escaped(&mut writer, created)?;
            write!(writer, "</name>")?;
        }
        writeln!(writer, "<trkseg>")?;
        for sample in self.samples() {
            let row = sample.row_index();
            let Some(point) = self.geo_point(row) else {
                continue;
            };
            write!(
                writer,
                "<trkpt lat=\"{}\" lon=\"{}\"><extensions><vbo:row>{row}</vbo:row>",
                point.latitude_deg, point.longitude_deg
            )?;
            if let Some(seconds) = self.time_seconds(row) {
                write!(writer, "<vbo:time-seconds>{seconds}</vbo:time-seconds>")?;
            }
            writeln!(writer, "</extensions></trkpt>")?;
        }
        writer.write_all(b"</trkseg></trk></gpx>\n")?;
        Ok(())
    }

    /// Writes numeric channels as a typed Apache Parquet file.
    ///
    /// This is feature-gated because Apache Parquet has a substantial dependency footprint.
    /// Fields are named `channel_<index>` to prevent source channel names from being interpreted
    /// as Parquet schema syntax. Column labels are therefore their zero-based source indices.
    #[cfg(feature = "parquet")]
    pub fn write_parquet<W: Write + Send>(&self, writer: W) -> Result<(), ExportError> {
        use std::sync::Arc;

        use parquet::{
            basic::Compression,
            data_type::DoubleType,
            file::{properties::WriterProperties, writer::SerializedFileWriter},
            schema::parser::parse_message_type,
        };

        ensure_channels(self)?;
        let fields = (0..self.channels.len())
            .map(|index| format!("REQUIRED DOUBLE channel_{index};"))
            .collect::<Vec<_>>()
            .join(" ");
        let schema = Arc::new(parse_message_type(&format!("message vbo {{ {fields} }}"))?);
        let properties = Arc::new(
            WriterProperties::builder()
                .set_compression(Compression::UNCOMPRESSED)
                .build(),
        );
        let mut file = SerializedFileWriter::new(writer, schema, properties)?;
        let width = self.channels.len();

        for first_row in (0..self.rows).step_by(PARQUET_ROW_GROUP_SIZE) {
            let rows = (self.rows - first_row).min(PARQUET_ROW_GROUP_SIZE);
            let mut group = file.next_row_group()?;
            for column in 0..width {
                let mut values = Vec::with_capacity(rows);
                for row in first_row..first_row + rows {
                    values.push(self.values[row * width + column]);
                }
                let Some(mut output) = group.next_column()? else {
                    return Err(ExportError::NoChannels);
                };
                output
                    .typed::<DoubleType>()
                    .write_batch(&values, None, None)?;
                output.close()?;
            }
            group.close()?;
        }
        file.close()?;
        Ok(())
    }
}

fn ensure_channels(vbo: &Vbo) -> Result<(), ExportError> {
    (!vbo.channels.is_empty())
        .then_some(())
        .ok_or(ExportError::NoChannels)
}

fn write_xml_escaped(mut writer: impl Write, value: &str) -> io::Result<()> {
    for part in value.split_inclusive(['&', '<', '>', '\"', '\'']) {
        let (body, delimiter) =
            part.char_indices()
                .last()
                .map_or((part, None), |(index, character)| {
                    if matches!(character, '&' | '<' | '>' | '\"' | '\'') {
                        (&part[..index], Some(character))
                    } else {
                        (part, None)
                    }
                });
        writer.write_all(body.as_bytes())?;
        if let Some(delimiter) = delimiter {
            writer.write_all(match delimiter {
                '&' => b"&amp;",
                '<' => b"&lt;",
                '>' => b"&gt;",
                '\"' => b"&quot;",
                '\'' => b"&apos;",
                _ => unreachable!("delimiter is restricted above"),
            })?;
        }
    }
    Ok(())
}
