use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

use crate::core::domain::assets::asset::value_objects::AssetId;

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
    /// Optional link to the asset holding the station's image. The station
    /// **owns** the link (the asset repository is station-agnostic); it is set
    /// by the import (provider image or the built-in default).
    pub image_asset_id: Option<AssetId>,
    /// Persisted provider image hash used for hash-based change detection during
    /// import. `None` when the provider reports no image (built-in default).
    pub image_sha256: Option<String>,
    /// Lifecycle status: `Active` while the provider still serves the station,
    /// `Inactive` when a provider update stopped including it.
    pub status: value_objects::Status,
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

    /// Lifecycle status of a counting station. `Active` is the default; a
    /// station becomes `Inactive` when a provider update stops including it in
    /// its station output.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub enum Status {
        #[default]
        Active,
        Inactive,
    }

    impl Status {
        /// The persisted/lowercase form used in the database and the BFF.
        pub fn as_str(&self) -> &'static str {
            match self {
                Status::Active => "active",
                Status::Inactive => "inactive",
            }
        }

        /// Parses the persisted lowercase form. Unknown values are rejected so
        /// the DB CHECK constraint and this enum can never drift silently.
        pub fn parse(value: &str) -> Result<Self, DomainError> {
            match value {
                "active" => Ok(Status::Active),
                "inactive" => Ok(Status::Inactive),
                _ => Err(DomainError::InvalidQuery(format!(
                    "unknown station status '{value}'"
                ))),
            }
        }
    }

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
    previous_local_days(tz, now, 1)
}

/// The `n` complete local days that end `offset_days` days before today in `tz`,
/// as a closed UTC interval `(from, to)` (inclusive upper bound, consistent with
/// [`previous_local_day`] and
/// [`MeasurementRepository::sum`](crate::core::domain::measurements::repository_port::MeasurementRepository)).
///
/// `from` is the local midnight `offset_days + n` days before today, `to` is one
/// microsecond *before* the local midnight `offset_days` days before today.
/// `offset_days = 0` yields the `n` days ending yesterday (the same window as
/// [`previous_local_days`]); a non-zero offset moves the whole window further
/// back (used e.g. for the previous period of the last complete day or the last
/// 30 days). DST-aware: each day is 23 h or 25 h long on the days the clocks
/// change.
pub fn local_days_window(
    tz: Tz,
    now: DateTime<Utc>,
    n: u32,
    offset_days: u32,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    if n == 0 {
        return Err(crate::core::domain::error::DomainError::InvalidQuery(
            "local_days_window requires n >= 1".to_string(),
        ));
    }
    let now_local = now.with_timezone(&tz);
    let today = now_local.date_naive();
    let to_date = today
        .checked_sub_signed(chrono::Duration::days(i64::from(offset_days)))
        .ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery(
                "cannot compute the previous days (date out of range)".to_string(),
            )
        })?;
    let from_date = to_date
        .checked_sub_signed(chrono::Duration::days(i64::from(n)))
        .ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery(
                "cannot compute the previous days (date out of range)".to_string(),
            )
        })?;
    let from = local_midnight_utc(tz, from_date)?;
    let to = local_midnight_utc(tz, to_date)? - chrono::Duration::microseconds(1);
    Ok((from, to))
}

/// The `n` complete local days immediately before today in `tz` as a closed UTC
/// interval `(from, to)` (inclusive upper bound, consistent with
/// [`previous_local_day`] and
/// [`MeasurementRepository::sum`](crate::core::domain::measurements::repository_port::MeasurementRepository)).
///
/// `from` is the local midnight `n` days before today, `to` is one microsecond
/// *before* today's local midnight. DST-aware: each day is 23 h or 25 h long on
/// the days the clocks change.
pub fn previous_local_days(
    tz: Tz,
    now: DateTime<Utc>,
    n: u32,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    local_days_window(tz, now, n, 0)
}

/// The previous complete calendar month in `tz` as a closed UTC interval
/// `(from, to)` (inclusive upper bound). `from` is local midnight on the 1st of
/// the previous month, `to` is one microsecond *before* local midnight on the
/// 1st of the current month.
pub fn previous_calendar_month(
    tz: Tz,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    calendar_month_window(tz, now, 1)
}

/// The complete calendar month that is `months_back` months before the month
/// containing `now`, as a closed UTC interval. `months_back = 1` is the
/// previous calendar month, `months_back = 2` the one before it (used as the
/// comparison period for the month metric's trend).
pub fn calendar_month_window(
    tz: Tz,
    now: DateTime<Utc>,
    months_back: u32,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    if months_back == 0 {
        return Err(crate::core::domain::error::DomainError::InvalidQuery(
            "calendar_month_window requires months_back >= 1".to_string(),
        ));
    }
    let now_local = now.with_timezone(&tz);
    let today = now_local.date_naive();
    let first_of_current = today.with_day(1).ok_or_else(|| {
        crate::core::domain::error::DomainError::InvalidQuery(
            "cannot compute the first day of the month".to_string(),
        )
    })?;
    let from = local_midnight_utc(tz, months_before(first_of_current, months_back)?)?;
    let to = local_midnight_utc(tz, months_before(first_of_current, months_back - 1)?)?
        - chrono::Duration::microseconds(1);
    Ok((from, to))
}

/// The 1st of the month that is `months_back` months before `date`.
fn months_before(
    date: NaiveDate,
    months_back: u32,
) -> Result<NaiveDate, crate::core::domain::error::DomainError> {
    let first_of_month = date.with_day(1).ok_or_else(|| {
        crate::core::domain::error::DomainError::InvalidQuery(
            "cannot compute the first day of the month".to_string(),
        )
    })?;
    first_of_month
        .checked_sub_months(chrono::Months::new(months_back))
        .ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery(
                "cannot compute the target month (date out of range)".to_string(),
            )
        })
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

/// The complete calendar year that is `years_back` years before the year
/// containing `now`, as a closed UTC interval. `years_back = 1` is the previous
/// calendar year, `years_back = 2` the one before it (comparison period).
pub fn calendar_year_window(
    tz: Tz,
    now: DateTime<Utc>,
    years_back: u32,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    if years_back == 0 {
        return Err(crate::core::domain::error::DomainError::InvalidQuery(
            "calendar_year_window requires years_back >= 1".to_string(),
        ));
    }
    let now_local = now.with_timezone(&tz);
    let current_year = now_local.year();
    let from_date =
        NaiveDate::from_ymd_opt(current_year - years_back as i32, 1, 1).ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery("invalid target year".to_string())
        })?;
    let to_date =
        NaiveDate::from_ymd_opt(current_year - years_back as i32 + 1, 1, 1).ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery("invalid target year".to_string())
        })?;
    let from = local_midnight_utc(tz, from_date)?;
    let to = local_midnight_utc(tz, to_date)? - chrono::Duration::microseconds(1);
    Ok((from, to))
}

/// The previous complete calendar year in `tz` as a closed UTC interval
/// (`years_back = 1`), matching the year metric's trend period.
pub fn previous_calendar_year(
    tz: Tz,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), crate::core::domain::error::DomainError> {
    calendar_year_window(tz, now, 1)
}

/// The UTC instant of local midnight on Jan 1 of the year containing `now` in
/// `tz` (the alignment origin for 1-day bucketing over a year).
pub fn local_year_start(
    tz: Tz,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, crate::core::domain::error::DomainError> {
    let now_local = now.with_timezone(&tz);
    let jan_first = NaiveDate::from_ymd_opt(now_local.year(), 1, 1).ok_or_else(|| {
        crate::core::domain::error::DomainError::InvalidQuery("invalid year".to_string())
    })?;
    local_midnight_utc(tz, jan_first)
}

/// The UTC instant of local midnight on the Monday of the ISO week containing
/// `now` in `tz` (the alignment origin for 15-minute bucketing over a week).
pub fn local_week_start(
    tz: Tz,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, crate::core::domain::error::DomainError> {
    let now_local = now.with_timezone(&tz);
    let days_since_monday = now_local.weekday().num_days_from_monday() as i64;
    let monday = now_local
        .date_naive()
        .checked_sub_signed(chrono::Duration::days(days_since_monday))
        .ok_or_else(|| {
            crate::core::domain::error::DomainError::InvalidQuery(
                "cannot compute the week start (date out of range)".to_string(),
            )
        })?;
    local_midnight_utc(tz, monday)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use chrono_tz::{Europe::Berlin, Tz};

    use super::{
        calendar_month_window, calendar_year_window, local_days_window, local_midnight_utc,
        local_week_start, local_year_start, previous_calendar_month, previous_calendar_year,
        previous_local_day, previous_local_days,
    };
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

    #[test]
    fn calendar_month_window_rejects_out_of_range_months_back() {
        // `months_before` cannot step back u32::MAX months; the defensive
        // `checked_sub_months` guard must surface an InvalidQuery error.
        let result = calendar_month_window(Berlin, utc(2024, 1, 15, 12, 0, 0), u32::MAX);
        assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
    }

    #[test]
    fn previous_local_days_returns_the_last_n_complete_days() {
        // Berlin 2024-01-11 (CET): the previous 7 complete days (Jan 4..10)
        // span UTC [2024-01-03T23:00:00Z, 2024-01-10T23:00:00Z).
        let (from, to) = previous_local_days(Berlin, utc(2024, 1, 11, 12, 0, 0), 7).unwrap();
        assert_eq!(from, utc(2024, 1, 3, 23, 0, 0));
        assert_eq!(to, utc(2024, 1, 10, 23, 0, 0) - Duration::microseconds(1));
        assert_eq!(
            to - from,
            Duration::hours(7 * 24) - Duration::microseconds(1)
        );
    }

    #[test]
    fn previous_local_days_matches_previous_local_day_for_one() {
        let (from, to) = previous_local_days(Berlin, utc(2024, 1, 2, 12, 0, 0), 1).unwrap();
        assert_eq!(
            (from, to),
            previous_local_day(Berlin, utc(2024, 1, 2, 12, 0, 0)).unwrap()
        );
    }

    #[test]
    fn previous_local_days_spans_dst_transition_within_the_window() {
        // The 7 complete days up to 2024-04-02 include the 2024-03-31 spring
        // forward, so the window is 7*24h - 1h long (23h day included).
        let (from, to) = previous_local_days(Berlin, utc(2024, 4, 2, 12, 0, 0), 7).unwrap();
        assert_eq!(from, utc(2024, 3, 25, 23, 0, 0));
        assert_eq!(to, utc(2024, 4, 1, 22, 0, 0) - Duration::microseconds(1));
        assert_eq!(
            to - from,
            Duration::hours(7 * 24 - 1) - Duration::microseconds(1)
        );
    }

    #[test]
    fn previous_local_days_rejects_zero() {
        assert!(matches!(
            previous_local_days(Berlin, utc(2024, 1, 2, 12, 0, 0), 0),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn local_days_window_with_offset_returns_the_days_before_the_previous_window() {
        // Berlin (CET, UTC+1): for 2024-01-11 the day before the previous
        // complete day is the local day 2024-01-09,
        // i.e. [2024-01-08T23:00:00Z, 2024-01-09T23:00:00Z).
        let (from, to) = local_days_window(Berlin, utc(2024, 1, 11, 12, 0, 0), 1, 1).unwrap();
        assert_eq!(from, utc(2024, 1, 8, 23, 0, 0));
        assert_eq!(to, utc(2024, 1, 9, 23, 0, 0) - Duration::microseconds(1));

        // 30 days ending 30 days ago (the period before the last 30 complete
        // days): for 2024-01-11 that is the local days 2023-11-12..2023-12-11,
        // i.e. [2023-11-11T23:00:00Z, 2023-12-11T23:00:00Z).
        let (from30, to30) = local_days_window(Berlin, utc(2024, 1, 11, 12, 0, 0), 30, 30).unwrap();
        assert_eq!(from30, utc(2023, 11, 11, 23, 0, 0));
        assert_eq!(
            to30,
            utc(2023, 12, 11, 23, 0, 0) - Duration::microseconds(1)
        );
    }

    #[test]
    fn local_days_window_errors_on_date_out_of_range() {
        // chrono's earliest date is year -262143-01-01; subtracting one more day
        // overflows the checked subtraction. offset_days = 1 fails in `to_date`,
        // offset_days = 0 keeps `to_date` valid but fails in `from_date`, so both
        // error closures are exercised.
        let min_date_now = Utc
            .with_ymd_and_hms(-262_143, 1, 1, 0, 0, 0)
            .single()
            .unwrap();
        let result_to = local_days_window(Berlin, min_date_now, 1, 1);
        assert!(matches!(result_to, Err(DomainError::InvalidQuery(_))));
        let result_from = local_days_window(Berlin, min_date_now, 1, 0);
        assert!(matches!(result_from, Err(DomainError::InvalidQuery(_))));
    }

    #[test]
    fn previous_calendar_month_spans_the_previous_full_month() {
        // Previous full month for 2024-04-15 (CEST, UTC+2) is March:
        // [2024-02-29T23:00:00Z, 2024-03-31T22:00:00Z).
        let (from, to) = previous_calendar_month(Berlin, utc(2024, 4, 15, 12, 0, 0)).unwrap();
        assert_eq!(from, utc(2024, 2, 29, 23, 0, 0));
        assert_eq!(to, utc(2024, 3, 31, 22, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn calendar_month_window_two_back_is_the_month_before_the_previous_one() {
        // months_back=2 for April is February: [2024-01-31T23:00:00Z, 2024-02-29T23:00:00Z).
        let (from, to) = calendar_month_window(Berlin, utc(2024, 4, 15, 12, 0, 0), 2).unwrap();
        assert_eq!(from, utc(2024, 1, 31, 23, 0, 0));
        assert_eq!(to, utc(2024, 2, 29, 23, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn calendar_month_window_handles_january_wrap() {
        // For 2024-01-15, months_back=1 is December 2023 (CET, UTC+1):
        // [2023-11-30T23:00:00Z, 2023-12-31T23:00:00Z).
        let (from, to) = calendar_month_window(Berlin, utc(2024, 1, 15, 12, 0, 0), 1).unwrap();
        assert_eq!(from, utc(2023, 11, 30, 23, 0, 0));
        assert_eq!(to, utc(2023, 12, 31, 23, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn calendar_month_window_rejects_zero() {
        assert!(matches!(
            calendar_month_window(Berlin, utc(2024, 4, 15, 12, 0, 0), 0),
            Err(DomainError::InvalidQuery(_))
        ));
    }

    #[test]
    fn local_week_start_returns_the_monday_of_the_current_local_week() {
        // 2024-01-04 is a Thursday in Berlin (CET, UTC+1); the ISO week starts
        // on Monday 2024-01-01 00:00 CET = 2023-12-31 23:00 UTC.
        let monday = local_week_start(Berlin, utc(2024, 1, 4, 12, 0, 0)).unwrap();
        assert_eq!(monday, utc(2023, 12, 31, 23, 0, 0));

        // A Monday at noon is the start of that same week (zero days back).
        let on_monday = local_week_start(Berlin, utc(2024, 1, 1, 12, 0, 0)).unwrap();
        assert_eq!(on_monday, utc(2023, 12, 31, 23, 0, 0));

        // A Sunday is 6 days after the Monday of its week.
        let sunday = local_week_start(Berlin, utc(2024, 1, 7, 12, 0, 0)).unwrap();
        assert_eq!(sunday, utc(2023, 12, 31, 23, 0, 0));
    }

    #[test]
    fn local_year_start_returns_jan_first_local_midnight() {
        // Berlin 2024 (CET) Jan 1 00:00 = 2023-12-31 23:00 UTC.
        assert_eq!(
            local_year_start(Berlin, utc(2024, 4, 15, 12, 0, 0)).unwrap(),
            utc(2023, 12, 31, 23, 0, 0)
        );
        assert_eq!(
            local_year_start(Berlin, utc(2023, 1, 1, 1, 0, 0)).unwrap(),
            utc(2022, 12, 31, 23, 0, 0)
        );
    }

    #[test]
    fn previous_calendar_year_spans_the_previous_full_year() {
        // Previous full year for 2024-04-15 (CET) is 2023: Berlin year 2023
        // spans UTC [2022-12-31T23:00:00Z, 2023-12-31T23:00:00Z).
        let (from, to) = previous_calendar_year(Berlin, utc(2024, 4, 15, 12, 0, 0)).unwrap();
        assert_eq!(from, utc(2022, 12, 31, 23, 0, 0));
        assert_eq!(to, utc(2023, 12, 31, 23, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn calendar_year_window_two_back_is_the_year_before_the_previous_one() {
        // years_back=2 for April 2024 is 2022: [2021-12-31T23:00:00Z, 2022-12-31T23:00:00Z).
        let (from, to) = calendar_year_window(Berlin, utc(2024, 4, 15, 12, 0, 0), 2).unwrap();
        assert_eq!(from, utc(2021, 12, 31, 23, 0, 0));
        assert_eq!(to, utc(2022, 12, 31, 23, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn calendar_year_window_handles_january_wrap() {
        // For 2024-01-15, years_back=1 is 2023: [2022-12-31T23:00:00Z, 2023-12-31T23:00:00Z).
        let (from, to) = calendar_year_window(Berlin, utc(2024, 1, 15, 12, 0, 0), 1).unwrap();
        assert_eq!(from, utc(2022, 12, 31, 23, 0, 0));
        assert_eq!(to, utc(2023, 12, 31, 23, 0, 0) - Duration::microseconds(1));
    }

    #[test]
    fn calendar_year_window_rejects_zero() {
        assert!(matches!(
            calendar_year_window(Berlin, utc(2024, 4, 15, 12, 0, 0), 0),
            Err(DomainError::InvalidQuery(_))
        ));
    }
}
