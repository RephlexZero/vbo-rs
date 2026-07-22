use racelogic_vbo::{ParseOptions, Parser};

const VBO: &str = "File created Test & <recording>\n[header]\n[column names]\ntime lat long velocity\n[channel units]\nUTC DDMM.MMMM DDDMM.MMMM km/h\n[data]\n120000.0 5123.0000 00123.0000 10.0\n120001.0 5123.0600 00123.0600 20.0\n";

fn session() -> racelogic_vbo::Vbo {
    Parser::new(ParseOptions::default()).parse_str(VBO).unwrap()
}

#[cfg(feature = "csv")]
#[test]
fn csv_contains_channel_names_and_values() {
    let mut output = Vec::new();
    session().write_csv(&mut output).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "time,lat,long,velocity\n120000.0,5123.0,123.0,10.0\n120001.0,5123.06,123.06,20.0\n"
    );
}

#[cfg(feature = "gpx")]
#[test]
fn gpx_escapes_metadata_and_exports_conventional_coordinates() {
    let mut output = Vec::new();
    session().write_gpx(&mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains("<name>File created Test &amp; &lt;recording&gt;</name>"),
        "{output}"
    );
    assert!(output.contains("lat=\"51.38333333333333\" lon=\"-1.3833333333333333\""));
    assert_eq!(output.matches("<trkpt ").count(), 2);
}

#[cfg(feature = "gpx")]
#[test]
fn gpx_rejects_recordings_without_geographic_samples() {
    let vbo = Parser::new(ParseOptions::default())
        .parse_str("[column names]\ntime speed\n[data]\n120000 10\n")
        .unwrap();
    let error = vbo.write_gpx(Vec::new()).unwrap_err();
    assert!(error.to_string().contains("no valid latitude/longitude"));
}

#[cfg(feature = "serde")]
#[test]
fn serde_uses_a_rectangular_sample_matrix() {
    let value = serde_json::to_value(session()).unwrap();
    assert_eq!(value["samples"].as_array().unwrap().len(), 2);
    assert_eq!(value["samples"][0].as_array().unwrap().len(), 4);
}

#[cfg(feature = "parquet")]
#[test]
fn parquet_contains_all_rows_and_columns() {
    use parquet::{
        file::reader::{FileReader, SerializedFileReader},
        record::RowAccessor,
    };

    let mut output = Vec::new();
    session().write_parquet(&mut output).unwrap();
    let reader = SerializedFileReader::new(bytes::Bytes::from(output)).unwrap();
    assert_eq!(reader.metadata().file_metadata().num_rows(), 2);
    let rows = reader
        .get_row_iter(None)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!((rows[1].get_double(3).unwrap() - 20.0).abs() < f64::EPSILON);
}
