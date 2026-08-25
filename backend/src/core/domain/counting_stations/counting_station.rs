use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone)]
pub struct CountingStation {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
    /// External identifier in the source data (for change detection).
    pub external_datasource_id: Option<value_objects::ExternalDatasourceId>,
    /// Optional link to a persisted data source (nullable so renames never lose data).
    pub data_source_id: Option<value_objects::DataSourceId>,
    /// Optional GPS coordinates (WGS84 decimal degrees). `None` when the source
    /// does not provide them (the station is not shown on the map until patched).
    pub coordinates: Option<value_objects::GeoCoordinates>,
    /// IANA timezone the station's measurements are reported in (e.g.
    /// `Europe/Berlin`). The "last day" summary is computed in this timezone,
    /// DST-aware, so a provider may serve stations from several timezones.
    pub timezone: value_objects::Timezone,
}

pub mod value_objects {
    use chrono_tz::Tz;
    use uuid::Uuid;

    use crate::core::domain::error::DomainError;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Id(pub Uuid);
    #[derive(Debug, Clone)]
    pub struct Name(pub String);
    #[derive(Debug, Clone)]
    pub struct Description(pub String);
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ExternalDatasourceId(pub String);
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DataSourceId(pub Uuid);
    /// WGS84 GPS coordinates (latitude/longitude in decimal degrees).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct GeoCoordinates {
        pub latitude: f64,
        pub longitude: f64,
    }
    /// IANA timezone name (e.g. `Europe/Berlin`).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Timezone(pub String);

    impl Timezone {
        /// Resolves the IANA name to a `chrono_tz::Tz`, or `DomainError` when it
        /// is not a known timezone.
        pub fn parse(&self) -> Result<Tz, DomainError> {
            self.0.parse::<Tz>().map_err(|_| {
                DomainError::InvalidQuery(format!("unknown IANA timezone '{}'", self.0))
            })
        }
    }
}

/// Computes the previous complete local calendar day of `now` in `tz` as a
/// closed UTC interval `(from, to)` suitable for `MeasurementRepository::sum`,
/// which uses an inclusive upper bound.
///
/// `from` is yesterday's local midnight, `to` is one microsecond *before*
/// today's local midnight, so the effective window is
/// `[yesterday 00:00:00.000000, today 00:00:00)` in `tz` (the first measurement
/// of today is excluded). DST-aware: the window is 23 h or 25 h long on the
/// days the clocks change.
pub fn previous_local_day(
    tz: Tz,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    let now_local = now.with_timezone(&tz);
    let today = now_local.date_naive();
    let yesterday = today.pred_opt().ok_or_else(|| {
        crate::core::domain::error::DomainError::InvalidQuery(
            "cannot compute the previous day (date out of range)".to_string(),
        )
    })?;
    let from = local_midnight_utc(tz, yesterday)?;
    let to = local_midnight_utc(tz, today)? - chrono::Duration::microseconds(1);
    Ok((from, to))
}

/// Converts the local midnight of `date` in `tz` to its UTC instant. Errors when
/// the local time does not exist (a DST gap) or the date is invalid.
fn local_midnight_utc(
    tz: Tz,
    date: NaiveDate,
) -> Result<DateTime<Utc>, crate::core::domain::error::DomainError> {
    let naive = date.and_hms_opt(0, 0, 0).ok_or_else(|| {
        crate::core::domain::error::DomainError::InvalidQuery("invalid date".to_string())
    })?;
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|local| local.with_timezone(&Utc))
        .ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery(format!(
                "local midnight does not exist in timezone {tz}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use chrono_tz::{Europe::Berlin, Tz};

    use super::{local_midnight_utc, previous_local_day};
    use crate::core::domain::error::DomainError;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    #[test]
    fn timezone_parse_accepts_known_iana_names() {
        assert_eq!(
            super::value_objects::Timezone("Europe/Berlin".to_string())
                .parse()
                .unwrap(),
            Berlin
        );
    }

    #[test]
    fn timezone_parse_rejects_unknown_iana_names() {
        let error = super::value_objects::Timezone("Not/AZone".to_string())
            .parse()
            .unwrap_err();
        assert!(matches!(error, DomainError::InvalidQuery(_)));
    }

    #[test]
    fn previous_local_day_winter_returns_the_previous_utc_day() {
        // Berlin 2024-01-01 (CET, UTC+1) spans UTC
        // [2023-12-31T23:00:00Z, 2024-01-01T23:00:00Z).
        let (from, to) = previous_local_day(Berlin, utc(2024, 1, 2, 12, 0, 0)).unwrap();
        assert_eq!(from, utc(2023, 12, 31, 23, 0, 0));
        assert_eq!(to, utc(2024, 1, 1, 23, 0, 0) - Duration::microseconds(1));
        assert_eq!(to - from, Duration::hours(24) - Duration::microseconds(1));
    }

    #[test]
    fn previous_local_day_spring_forward_day_is_23_hours() {
        // DST 2024 starts 2024-03-31 02:00 CET -> 03:00 CEST, so the Berlin day
        // 2024-03-31 is only 23 h long: UTC
        // [2024-03-30T23:00:00Z, 2024-03-31T22:00:00Z).
        let (from, to) = previous_local_day(Berlin, utc(2024, 4, 1, 12, 0, 0)).unwrap();
        assert_eq!(from, utc(2024, 3, 30, 23, 0, 0));
        assert_eq!(to, utc(2024, 3, 31, 22, 0, 0) - Duration::microseconds(1));
        assert_eq!(to - from, Duration::hours(23) - Duration::microseconds(1));
    }

    #[test]
    fn previous_local_day_fall_back_day_is_25_hours() {
        // DST 2024 ends 2024-10-27 03:00 CEST -> 02:00 CET, so the Berlin day
        // 2024-10-27 is 25 h long: UTC
        // [2024-10-26T22:00:00Z, 2024-10-27T23:00:00Z).
        let (from, to) = previous_local_day(Berlin, utc(2024, 10, 28, 12, 0, 0)).unwrap();
        assert_eq!(from, utc(2024, 10, 26, 22, 0, 0));
        assert_eq!(to, utc(2024, 10, 27, 23, 0, 0) - Duration::microseconds(1));
        assert_eq!(to - from, Duration::hours(25) - Duration::microseconds(1));
    }

    #[test]
    fn previous_local_day_resolves_to_utc_for_other_timezones() {
        let tz: Tz = "America/New_York".parse().unwrap();
        let (from, to) = previous_local_day(tz, utc(2024, 1, 2, 12, 0, 0)).unwrap();
        // New York (EST, UTC-5) day 2024-01-01 spans UTC
        // [2024-01-01T05:00:00Z, 2024-01-02T05:00:00Z).
        assert_eq!(from, utc(2024, 1, 1, 5, 0, 0));
        assert_eq!(to, utc(2024, 1, 2, 5, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn local_midnight_utc_errors_on_a_midnight_dst_gap() {
        // America/Sao_Paulo historically started DST at local midnight
        // (2018-11-04 00:00 -> 01:00), so that local midnight does not exist.
        let tz: Tz = "America/Sao_Paulo".parse().unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2018, 11, 4).unwrap();
        let result = local_midnight_utc(tz, date);
        assert!(
            matches!(result, Err(DomainError::InvalidQuery(_))),
            "a DST gap at midnight must be an error, got {result:?}"
        );
    }
}
