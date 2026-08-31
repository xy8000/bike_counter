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

use super::resolution::has_full_coverage;
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
    repository.sum(from, to, channel_ids, None)
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
///
/// When `exclude_new_stations` is on (the Bike-Trends setting):
/// - **multi-station** (summary): a station is skipped for a metric unless it
///   has measurements covering the whole current **and** previous window of that
///   metric (like-for-like — the "new station skew" disappears);
/// - **single-station** (detail): the totals are kept but `is_new` is set when
///   the station lacks that full coverage, so the UI can show a neutral "New"
///   indicator instead of a misleading trend arrow.
///
/// The coverage check is generic over any `(from, to)` window (see
/// [`has_full_coverage`]), so a future custom from/to date picker reuses it
/// unchanged.
pub(super) fn metric_windows(
    repository: &dyn MeasurementRepository,
    stations: &[CountingStation],
    channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
    now: DateTime<Utc>,
    exclude_new_stations: bool,
) -> Result<Vec<MetricWindow>, DomainError> {
    let single_station = stations.len() == 1;
    let mut current = [0i64; 4];
    let mut previous = [0i64; 4];
    let mut is_new = [false; 4];

    for station in stations {
        let tz: Tz = station.timezone.parse()?;
        let channel_ids: Vec<ChannelId> = channels_by_station
            .get(&station.id.0)
            .map(|channels| channels.iter().map(|c| ChannelId(c.id.0)).collect())
            .unwrap_or_default();

        let (day_from, day_to) = previous_local_days(tz, now, 1)?;
        let (before_day_from, _) = previous_local_days(tz, now, 2)?;
        let day_previous_to = day_from - Duration::microseconds(1);

        let (week_from, week_to) = previous_local_days(tz, now, 7)?;
        let (before_week_from, _) = previous_local_days(tz, now, 14)?;
        let week_previous_to = week_from - Duration::microseconds(1);

        let (month_from, month_to) = previous_calendar_month(tz, now)?;
        let (before_month_from, _) = calendar_month_window(tz, now, 2)?;
        let month_previous_to = month_from - Duration::microseconds(1);

        let (year_from, year_to) = previous_calendar_year(tz, now)?;
        let (before_year_from, _) = calendar_year_window(tz, now, 2)?;
        let year_previous_to = year_from - Duration::microseconds(1);

        // One coverage query over the union of all eight windows, so every
        // metric's current + previous window can be checked in one pass (the
        // earliest `from` is the year-before-previous start, the latest `to` is
        // the most recent local day end). Skipped entirely unless the setting is
        // on, keeping the default path free of extra queries.
        let coverage = if exclude_new_stations {
            let union_to = day_to.max(week_to).max(month_to).max(year_to);
            repository.resolution_coverage(before_year_from, union_to, &channel_ids)?
        } else {
            Vec::new()
        };

        // The (current, previous) window pair of each metric, in MetricKey order.
        let windows = [
            ((day_from, day_to), (before_day_from, day_previous_to)),
            ((week_from, week_to), (before_week_from, week_previous_to)),
            (
                (month_from, month_to),
                (before_month_from, month_previous_to),
            ),
            ((year_from, year_to), (before_year_from, year_previous_to)),
        ];

        for (idx, ((c_from, c_to), (p_from, p_to))) in windows.into_iter().enumerate() {
            if exclude_new_stations {
                let covers_current = has_full_coverage(&coverage, c_from, c_to, now);
                let covers_previous = has_full_coverage(&coverage, p_from, p_to, now);
                if single_station {
                    if !covers_current || !covers_previous {
                        is_new[idx] = true;
                    }
                } else if !covers_current || !covers_previous {
                    continue;
                }
            }
            current[idx] += sum_window(repository, c_from, c_to, &channel_ids)?;
            previous[idx] += sum_window(repository, p_from, p_to, &channel_ids)?;
        }
    }

    Ok(vec![
        MetricWindow {
            key: MetricKey::LastDay,
            current: current[0],
            previous: previous[0],
            is_new: single_station && is_new[0],
        },
        MetricWindow {
            key: MetricKey::Last7Days,
            current: current[1],
            previous: previous[1],
            is_new: single_station && is_new[1],
        },
        MetricWindow {
            key: MetricKey::LastMonth,
            current: current[2],
            previous: previous[2],
            is_new: single_station && is_new[2],
        },
        MetricWindow {
            key: MetricKey::LastYear,
            current: current[3],
            previous: previous[3],
            is_new: single_station && is_new[3],
        },
    ])
}
