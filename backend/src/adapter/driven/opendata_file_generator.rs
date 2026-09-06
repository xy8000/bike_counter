//! Driven adapter implementing [`OpenDataFileGenerator`]: serializes export rows
//! into a full parquet, gzip-compressed CSV or JSON distribution file. The core
//! only asks for bytes; this adapter owns every format detail.
//!
//! The CSV/JSON/parquet column layout is identical and matches the documented
//! measurement schema:
//!
//! `station_id`, `channel_id`, `channel_name`, `timestamp` (naive central-Europe
//! local, no offset, `YYYY-MM-DDTHH:MM:SS`), `value`, `resolution_seconds`.

use std::sync::Arc;

use arrow::array::{Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::core::domain::error::DomainError;
use crate::core::domain::opendata::file::Format;
use crate::core::domain::opendata::file_generator_port::OpenDataFileGenerator;
use crate::core::domain::opendata::measurement::{OpenDataMeasurement, format_timestamp};

/// A [`OpenDataFileGenerator`] that emits every format. Stateless.
#[derive(Debug, Default)]
pub struct OpendataFileGenerator;

/// Headers / JSON keys of the export schema, shared across the formats.
const STATION_ID: &str = "station_id";
const CHANNEL_ID: &str = "channel_id";
const CHANNEL_NAME: &str = "channel_name";
const TIMESTAMP: &str = "timestamp";
const VALUE: &str = "value";
const RESOLUTION_SECONDS: &str = "resolution_seconds";

impl OpenDataFileGenerator for OpendataFileGenerator {
    fn generate(
        &self,
        rows: &[OpenDataMeasurement],
        format: Format,
    ) -> Result<Vec<u8>, DomainError> {
        match format {
            Format::Parquet => to_parquet(rows),
            Format::CsvGz => to_csv_gz(rows),
            Format::Json => to_json(rows),
        }
    }
}

fn internal_error(format: Format, error: impl std::fmt::Display) -> DomainError {
    DomainError::Database(format!(
        "failed to generate the {} file: {error}",
        format.as_str()
    ))
}

/// Serializes the rows into a JSON array of record objects.
fn to_json(rows: &[OpenDataMeasurement]) -> Result<Vec<u8>, DomainError> {
    let records: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                STATION_ID: row.station_id,
                CHANNEL_ID: row.channel_id,
                CHANNEL_NAME: row.channel_name,
                TIMESTAMP: format_timestamp(row.timestamp),
                VALUE: row.value,
                RESOLUTION_SECONDS: row.resolution_seconds,
            })
        })
        .collect();
    serde_json::to_vec(&records).map_err(|error| internal_error(Format::Json, error))
}

/// Serializes the rows into a gzip-compressed CSV (header + one line per row).
fn to_csv_gz(rows: &[OpenDataMeasurement]) -> Result<Vec<u8>, DomainError> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut writer = csv::Writer::from_writer(&mut encoder);
        let header = [
            STATION_ID,
            CHANNEL_ID,
            CHANNEL_NAME,
            TIMESTAMP,
            VALUE,
            RESOLUTION_SECONDS,
        ];
        writer
            .write_record(header)
            .map_err(|error| internal_error(Format::CsvGz, error))?;
        for row in rows {
            let record = [
                row.station_id.to_string(),
                row.channel_id.to_string(),
                row.channel_name.clone(),
                format_timestamp(row.timestamp),
                row.value.to_string(),
                row.resolution_seconds.to_string(),
            ];
            writer
                .write_record(record)
                .map_err(|error| internal_error(Format::CsvGz, error))?;
        }
        writer
            .flush()
            .map_err(|error| internal_error(Format::CsvGz, error))?;
    }
    encoder
        .finish()
        .map_err(|error| internal_error(Format::CsvGz, error))
}

/// Serializes the rows into a columnar snappy-compressed parquet file whose
/// columns match the CSV/JSON schema (UUIDs and the naive timestamp as strings
/// so all three formats stay byte-identical in meaning).
fn to_parquet(rows: &[OpenDataMeasurement]) -> Result<Vec<u8>, DomainError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new(STATION_ID, DataType::Utf8, false),
        Field::new(CHANNEL_ID, DataType::Utf8, false),
        Field::new(CHANNEL_NAME, DataType::Utf8, false),
        Field::new(TIMESTAMP, DataType::Utf8, false),
        Field::new(VALUE, DataType::Int64, false),
        Field::new(RESOLUTION_SECONDS, DataType::Int64, false),
    ]));

    let station_ids: Vec<String> = rows.iter().map(|r| r.station_id.to_string()).collect();
    let channel_ids: Vec<String> = rows.iter().map(|r| r.channel_id.to_string()).collect();
    let channel_names: Vec<&str> = rows.iter().map(|r| r.channel_name.as_str()).collect();
    let timestamps: Vec<String> = rows.iter().map(|r| format_timestamp(r.timestamp)).collect();
    let values: Vec<i64> = rows.iter().map(|r| r.value).collect();
    let resolutions: Vec<i64> = rows.iter().map(|r| r.resolution_seconds).collect();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(station_ids)),
            Arc::new(StringArray::from(channel_ids)),
            Arc::new(StringArray::from(channel_names)),
            Arc::new(StringArray::from(timestamps)),
            Arc::new(Int64Array::from(values)),
            Arc::new(Int64Array::from(resolutions)),
        ],
    )
    .map_err(|error| internal_error(Format::Parquet, error))?;

    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(Vec::new(), schema, Some(properties))
        .map_err(|error| internal_error(Format::Parquet, error))?;
    writer
        .write(&batch)
        .map_err(|error| internal_error(Format::Parquet, error))?;
    // `into_inner` flushes the outstanding data, finalizes the footer and
    // returns the underlying `Vec<u8>`.
    writer
        .into_inner()
        .map_err(|error| internal_error(Format::Parquet, error))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use chrono::NaiveDate;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use super::*;

    fn row(value: i64) -> OpenDataMeasurement {
        OpenDataMeasurement {
            station_id: uuid::Uuid::from_u128(1),
            channel_id: uuid::Uuid::from_u128(2),
            channel_name: "Channel A".to_string(),
            timestamp: NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap(),
            value,
            resolution_seconds: 3600,
        }
    }

    #[test]
    fn json_round_trips_the_rows() {
        let generator = OpendataFileGenerator;
        let bytes = generator
            .generate(&[row(42), row(7)], Format::Json)
            .unwrap();
        let records: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let records = records.as_array().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["value"], 42);
        assert_eq!(records[0]["timestamp"], "2026-09-05T14:00:00");
        assert_eq!(
            records[0]["station_id"],
            uuid::Uuid::from_u128(1).to_string()
        );
        assert_eq!(records[0]["resolution_seconds"], 3600);
        assert!(records[0].get("channel_name").is_some());
    }

    #[test]
    fn csv_gz_decompresses_to_a_csv_with_header() {
        let generator = OpendataFileGenerator;
        let bytes = generator.generate(&[row(5)], Format::CsvGz).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        let mut text = String::new();
        decoder.read_to_string(&mut text).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "station_id,channel_id,channel_name,timestamp,value,resolution_seconds"
        );
        assert!(lines[1].contains(",5,3600"));
        assert!(lines[1].contains("2026-09-05T14:00:00"));
    }

    #[test]
    fn empty_rows_produce_valid_files() {
        let generator = OpendataFileGenerator;
        assert_eq!(generator.generate(&[], Format::Json).unwrap(), b"[]");
        let csv = generator.generate(&[], Format::CsvGz).unwrap();
        assert!(!csv.is_empty());
        let parquet = generator.generate(&[], Format::Parquet).unwrap();
        // Parquet files start with the "PAR1" magic bytes.
        assert_eq!(&parquet[0..4], b"PAR1");
    }

    #[test]
    fn parquet_starts_with_magic() {
        let generator = OpendataFileGenerator;
        let bytes = generator.generate(&[row(1)], Format::Parquet).unwrap();
        assert_eq!(&bytes[0..4], b"PAR1");
        assert_eq!(&bytes[bytes.len() - 4..], b"PAR1");
    }

    #[test]
    fn parquet_columns_are_snappy_compressed() {
        let generator = OpendataFileGenerator;
        let bytes = generator
            .generate(&[row(1), row(2)], Format::Parquet)
            .unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(bytes)).unwrap();
        let columns = builder.metadata().row_groups()[0].columns();
        assert!(!columns.is_empty());
        for column in columns {
            assert_eq!(
                column.compression(),
                Compression::SNAPPY,
                "every column chunk must use snappy column compression"
            );
        }
    }
}
