//! Application helpers that compute the four overview metrics (last day, last 7
//! days, last month, last year) over one or more counting stations, each in its
//! own timezone.
//!
//! Consumed by the overview page and the station-summary page via
//! [`service::StationAnalyticsService`](super::service). The `now`-driven window
//! math here is a future seam for a date-picker: swapping "previous complete
//! period" for an arbitrary selected range touches only this module (and
//! [`super::graphs`]).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;

use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::{
    calendar_month_window, calendar_year_window, previous_calendar_month, previous_calendar_year,
    previous_local_days,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::MeasurementRepository;
use crate::core::domain::station_analytics::{MetricKey, MetricWindow};

/// Sums a window across every channel in one multi-channel query.
pub(super) fn sum_window(
    repository: &dyn MeasurementRepository,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    channel_ids: &[ChannelId],
) -> Result<i64, DomainError> {
    repository.sum(from, to, channel_ids)
}

/// Timestamp of the most recent successful data-source update.
pub(super) fn last_update(
    job_repository: &dyn JobRepository,
) -> Result<Option<DateTime<Utc>>, DomainError> {
    Ok(job_repository
        .find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)?
        .and_then(|job| job.finished_at))
}

/// The four aggregated overview metrics. Every station contributes its own
/// DST-aware windows, so a station's measurement counts in its own timezone.
/// With a single station this yields the overview-page metrics.
pub(super) fn metric_windows(
    repository: &dyn MeasurementRepository,
    stations: &[CountingStation],
    channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
    now: DateTime<Utc>,
) -> Result<Vec<MetricWindow>, DomainError> {
    let mut day = (0i64, 0i64);
    let mut week = (0i64, 0i64);
    let mut month = (0i64, 0i64);
    let mut year = (0i64, 0i64);

    for station in stations {
        let tz: Tz = station.timezone.parse()?;
        let channel_ids: Vec<ChannelId> = channels_by_station
            .get(&station.id.0)
            .map(|channels| channels.iter().map(|c| ChannelId(c.id.0)).collect())
            .unwrap_or_default();

        let (day_from, day_to) = previous_local_days(tz, now, 1)?;
        let (before_day_from, _) = previous_local_days(tz, now, 2)?;
        day.0 += sum_window(repository, day_from, day_to, &channel_ids)?;
        day.1 += sum_window(
            repository,
            before_day_from,
            day_from - Duration::microseconds(1),
            &channel_ids,
        )?;

        let (week_from, week_to) = previous_local_days(tz, now, 7)?;
        let (before_week_from, _) = previous_local_days(tz, now, 14)?;
        week.0 += sum_window(repository, week_from, week_to, &channel_ids)?;
        week.1 += sum_window(
            repository,
            before_week_from,
            week_from - Duration::microseconds(1),
            &channel_ids,
        )?;

        let (month_from, month_to) = previous_calendar_month(tz, now)?;
        let (before_month_from, _) = calendar_month_window(tz, now, 2)?;
        month.0 += sum_window(repository, month_from, month_to, &channel_ids)?;
        month.1 += sum_window(
            repository,
            before_month_from,
            month_from - Duration::microseconds(1),
            &channel_ids,
        )?;

        let (year_from, year_to) = previous_calendar_year(tz, now)?;
        let (before_year_from, _) = calendar_year_window(tz, now, 2)?;
        year.0 += sum_window(repository, year_from, year_to, &channel_ids)?;
        year.1 += sum_window(
            repository,
            before_year_from,
            year_from - Duration::microseconds(1),
            &channel_ids,
        )?;
    }

    Ok(vec![
        MetricWindow {
            key: MetricKey::LastDay,
            current: day.0,
            previous: day.1,
        },
        MetricWindow {
            key: MetricKey::Last7Days,
            current: week.0,
            previous: week.1,
        },
        MetricWindow {
            key: MetricKey::LastMonth,
            current: month.0,
            previous: month.1,
        },
        MetricWindow {
            key: MetricKey::LastYear,
            current: year.0,
            previous: year.1,
        },
    ])
}
