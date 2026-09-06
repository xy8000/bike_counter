//! The [`OpenDataFile`] aggregate: metadata of one published, immutable
//! distribution file (parquet / csv.gz / json) for a global or per-station
//! daily/monthly period. Only metadata is stored (in the `opendata_files`
//! table); the bytes live in the dedicated opendata object-storage bucket.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::error::DomainError;

/// How the file's period buckets the measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Granularity {
    /// One file per local calendar day; `period` is `YYYY-MM-DD`.
    Daily,
    /// One file per local calendar month; `period` is `YYYY-MM`.
    Monthly,
}

impl Granularity {
    /// The canonical wire / database representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Granularity::Daily => "daily",
            Granularity::Monthly => "monthly",
        }
    }
}

impl FromStr for Granularity {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "daily" => Ok(Granularity::Daily),
            "monthly" => Ok(Granularity::Monthly),
            _ => Err(DomainError::InvalidQuery(format!(
                "unknown opendata granularity '{value}'"
            ))),
        }
    }
}

/// The serialization format of a distribution file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Parquet,
    /// Gzip-compressed CSV.
    CsvGz,
    Json,
}

impl Format {
    /// Every format the export job produces.
    pub const ALL: [Format; 3] = [Format::Parquet, Format::CsvGz, Format::Json];

    /// The canonical wire / database representation (also the URL extension).
    pub fn as_str(&self) -> &'static str {
        match self {
            Format::Parquet => "parquet",
            Format::CsvGz => "csv.gz",
            Format::Json => "json",
        }
    }

    /// The HTTP `Content-Type` of the stored bytes.
    pub fn content_type(&self) -> &'static str {
        match self {
            Format::Parquet => "application/vnd.apache.parquet",
            Format::CsvGz => "application/gzip",
            Format::Json => "application/json",
        }
    }
}

impl FromStr for Format {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "parquet" => Ok(Format::Parquet),
            "csv.gz" => Ok(Format::CsvGz),
            "json" => Ok(Format::Json),
            _ => Err(DomainError::InvalidQuery(format!(
                "unknown opendata format '{value}'"
            ))),
        }
    }
}

/// Metadata of one published distribution file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDataFile {
    pub id: Uuid,
    /// Full object key in the opendata bucket (globally unique, immutable).
    pub object_key: String,
    /// `None` for global files, the counting-station UUID for per-station files.
    pub station_id: Option<Uuid>,
    pub granularity: Granularity,
    /// `YYYY-MM-DD` for daily, `YYYY-MM` for monthly.
    pub period: String,
    pub format: Format,
    /// Size of the stored bytes.
    pub byte_size: i64,
    /// SHA-256 hex digest of the stored bytes (used as the strong ETag).
    pub sha256: String,
    pub created_at: DateTime<Utc>,
}

/// Builds the deterministic object key for a distribution file, mirroring the
/// public URL layout:
///
/// - global daily:   `opendata/measurements/daily/{year}/{date}.{ext}`
/// - global monthly: `opendata/measurements/monthly/{year_month}/{year_month}.{ext}`
/// - station daily:  `opendata/stations/{uuid}/measurements/daily/{year}/{date}.{ext}`
/// - station monthly:`opendata/stations/{uuid}/measurements/monthly/{year_month}/{year_month}.{ext}`
///
/// The REST file endpoints reconstruct this from the URL path, so they can look
/// the row up by `object_key` without an extra indirection table.
pub fn object_key(
    station_id: Option<Uuid>,
    granularity: Granularity,
    period: &str,
    format: Format,
) -> String {
    // The first path segment under `measurements/{granularity}` equals the
    // period's year for daily files and the full period for monthly files.
    let year = period.split('-').next().unwrap_or(period);
    let period_dir = match granularity {
        Granularity::Daily => year,
        Granularity::Monthly => period,
    };
    let scope = match station_id {
        Some(station_id) => format!("opendata/stations/{station_id}/measurements"),
        None => "opendata/measurements".to_string(),
    };
    format!(
        "{scope}/{}/{}/{}.{}",
        granularity.as_str(),
        period_dir,
        period,
        format.as_str()
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn granularity_round_trips_through_string() {
        assert_eq!(Granularity::Daily.as_str(), "daily");
        assert_eq!(Granularity::Monthly.as_str(), "monthly");
        assert_eq!(Granularity::from_str("daily").unwrap(), Granularity::Daily);
        assert_eq!(
            Granularity::from_str("monthly").unwrap(),
            Granularity::Monthly
        );
        assert!(matches!(
            Granularity::from_str("hourly"),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn format_round_trips_through_string_and_exposes_content_type() {
        assert_eq!(Format::ALL.len(), 3);
        assert_eq!(Format::Parquet.as_str(), "parquet");
        assert_eq!(Format::CsvGz.as_str(), "csv.gz");
        assert_eq!(Format::Json.as_str(), "json");
        assert_eq!(
            Format::Parquet.content_type(),
            "application/vnd.apache.parquet"
        );
        assert_eq!(Format::CsvGz.content_type(), "application/gzip");
        assert_eq!(Format::Json.content_type(), "application/json");
        assert_eq!(Format::from_str("csv.gz").unwrap(), Format::CsvGz);
        assert_eq!(Format::from_str("json").unwrap(), Format::Json);
        assert_eq!(Format::from_str("parquet").unwrap(), Format::Parquet);
        assert!(matches!(
            Format::from_str("xml"),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn object_key_matches_the_public_url_layout() {
        let station = Uuid::from_u128(1);
        assert_eq!(
            object_key(None, Granularity::Daily, "2026-09-05", Format::Parquet),
            "opendata/measurements/daily/2026/2026-09-05.parquet"
        );
        assert_eq!(
            object_key(None, Granularity::Daily, "2026-09-05", Format::CsvGz),
            "opendata/measurements/daily/2026/2026-09-05.csv.gz"
        );
        assert_eq!(
            object_key(None, Granularity::Monthly, "2026-09", Format::Json),
            "opendata/measurements/monthly/2026-09/2026-09.json"
        );
        assert_eq!(
            object_key(
                Some(station),
                Granularity::Daily,
                "2026-09-05",
                Format::Json
            ),
            format!("opendata/stations/{station}/measurements/daily/2026/2026-09-05.json")
        );
        assert_eq!(
            object_key(
                Some(station),
                Granularity::Monthly,
                "2026-09",
                Format::Parquet
            ),
            format!("opendata/stations/{station}/measurements/monthly/2026-09/2026-09.parquet")
        );
    }
}
