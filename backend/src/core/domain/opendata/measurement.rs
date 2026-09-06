//! The export row the core hands to a file-generator adapter.
//!
//! Timestamps are **naive local central-European time** (`Europe/Berlin`, no UTC
//! offset), which is how they are serialized identically in every format
//! (parquet / csv.gz / json).

use chrono::NaiveDateTime;
use uuid::Uuid;

/// One measurement as it appears in an opendata file. The measurement identity
/// is denormalized onto the row (station + channel + channel name) so consumers
/// of a single file never need a join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDataMeasurement {
    pub station_id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    /// The count's interval start, in local central-European time, no offset.
    pub timestamp: NaiveDateTime,
    pub value: i64,
    /// Length of the counted interval in seconds (e.g. 300, 900, 3600, 86400).
    pub resolution_seconds: i64,
}

/// Formats a naive local timestamp as `YYYY-MM-DDTHH:MM:SS` (the shared string
/// form used in the JSON records, the CSV column and the parquet column).
pub fn format_timestamp(timestamp: NaiveDateTime) -> String {
    timestamp.format("%Y-%m-%dT%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn formats_a_naive_timestamp_without_offset() {
        let ts = NaiveDate::from_ymd_opt(2026, 9, 5)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        assert_eq!(format_timestamp(ts), "2026-09-05T14:00:00");
    }
}
