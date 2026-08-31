//! Application helpers that bucket measurement sums into the detail/summary
//! time-series graphs (day / week / last 30 days / year), the weekday and hour
//! radars and the pie charts.
//!
//! All window math lives here (see [`graph_windows`]); this is the future seam
//! for a date-picker: swapping the `now`-derived "previous complete period"
//! windows for an arbitrary selected range touches only this module (and
//! [`super::metrics`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Datelike, Duration, Utc};
use chrono_tz::Tz;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::{
    local_days_window, local_week_start, local_year_start, previous_calendar_year,
    previous_local_days,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{
    ChannelBucket, ChannelTotal, HourTotal, MeasurementRepository, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_analytics::{
    PerChannelSeries, PerStationSeries, PeriodGraphs, StationTotal, SummaryPeriodGraphs,
};

use super::resolution::{covers_whole_window, has_full_coverage};

/// Fixed bucket widths (seconds) used by the detail/summary graphs.
const SECONDS_PER_5_MINUTES: i64 = 5 * 60;
const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// One time-window: `[from, to]` with the bucket width and alignment origin.
pub(super) struct Window {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_seconds: i64,
    origin: DateTime<Utc>,
}

/// The (current, previous) window pair for one of the four selectable
/// timeframes.
pub(super) struct WindowPair {
    pub(super) current: Window,
    pub(super) previous: Window,
}

/// All four selectable timeframes, computed in one timezone.
pub(super) struct GraphWindows {
    pub(super) day: WindowPair,
    pub(super) week: WindowPair,
    pub(super) last_30_days: WindowPair,
    pub(super) year: WindowPair,
}

/// The aggregate + per-group data for one timeframe (a "group" is a channel on
/// the detail page or a station on the summary page).
pub(super) struct PeriodData {
    current: Vec<TimeBucket>,
    previous: Vec<TimeBucket>,
    weekday_radar: Vec<WeekdayTotal>,
    weekday_radar_previous: Vec<WeekdayTotal>,
    hourly: Vec<HourTotal>,
    hourly_previous: Vec<HourTotal>,
    per_group: Vec<GroupGraphData>,
}

/// The per-group data for one timeframe.
struct GroupGraphData {
    group_id: uuid::Uuid,
    current: Vec<TimeBucket>,
    previous: Vec<TimeBucket>,
    weekday_radar: Vec<WeekdayTotal>,
    weekday_radar_previous: Vec<WeekdayTotal>,
    hourly: Vec<HourTotal>,
    hourly_previous: Vec<HourTotal>,
    /// Total over the current period (pie slice).
    pie_total: i64,
}

/// The four (current, previous) window pairs shared by the detail and the
/// station-summary graphs, all as UTC instants.
pub(super) fn graph_windows(tz: Tz, now: DateTime<Utc>) -> Result<GraphWindows, DomainError> {
    let (day_from, day_to) = previous_local_days(tz, now, 1)?;
    let (previous_day_from, previous_day_to) = local_days_window(tz, now, 1, 1)?;
    let week_start = local_week_start(tz, now)?;
    let last_week_from = week_start - Duration::days(7);
    let last_week_to = week_start - Duration::microseconds(1);
    let (last_30_from, last_30_to) = previous_local_days(tz, now, 30)?;
    let (previous_30_from, previous_30_to) = local_days_window(tz, now, 30, 30)?;
    let year_start = local_year_start(tz, now)?;
    let (last_year_from, last_year_to) = previous_calendar_year(tz, now)?;

    Ok(GraphWindows {
        day: WindowPair {
            current: Window {
                from: day_from,
                to: day_to,
                bucket_seconds: SECONDS_PER_5_MINUTES,
                origin: day_from,
            },
            previous: Window {
                from: previous_day_from,
                to: previous_day_to,
                bucket_seconds: SECONDS_PER_5_MINUTES,
                origin: previous_day_from,
            },
        },
        week: WindowPair {
            current: Window {
                from: week_start,
                to: now,
                bucket_seconds: SECONDS_PER_HOUR,
                origin: week_start,
            },
            previous: Window {
                from: last_week_from,
                to: last_week_to,
                bucket_seconds: SECONDS_PER_HOUR,
                origin: last_week_from,
            },
        },
        last_30_days: WindowPair {
            current: Window {
                from: last_30_from,
                to: last_30_to,
                bucket_seconds: SECONDS_PER_DAY,
                origin: last_30_from,
            },
            previous: Window {
                from: previous_30_from,
                to: previous_30_to,
                bucket_seconds: SECONDS_PER_DAY,
                origin: previous_30_from,
            },
        },
        year: WindowPair {
            current: Window {
                from: year_start,
                to: now,
                bucket_seconds: SECONDS_PER_DAY,
                origin: year_start,
            },
            previous: Window {
                from: last_year_from,
                to: last_year_to,
                bucket_seconds: SECONDS_PER_DAY,
                origin: last_year_from,
            },
        },
    })
}

/// Folds buckets into per-weekday totals (ISO Mon = 1 .. Sun = 7) in `tz`.
/// Every bucket belongs to a single local weekday, so summing them yields
/// the correct weekday totals regardless of the bucket width (5 minutes,
/// 1 hour or 1 day). Only weekdays with traffic are returned.
fn weekday_totals(buckets: &[TimeBucket], tz: Tz) -> Vec<WeekdayTotal> {
    let mut totals = [0i64; 7];
    for bucket in buckets {
        let weekday = bucket
            .start
            .with_timezone(&tz)
            .weekday()
            .num_days_from_monday() as usize;
        totals[weekday] += bucket.total;
    }
    totals
        .iter()
        .enumerate()
        .filter(|(_, total)| **total > 0)
        .map(|(i, total)| WeekdayTotal {
            weekday: (i + 1) as u8,
            total: *total,
        })
        .collect()
}

/// The aggregate + per-group data for one timeframe. A "group" is a channel
/// (detail page) or a station (summary page); `group_of_channel` maps each
/// channel id to its group id and `group_order` gives the stable output
/// order. Everything (the aggregate series, the weekday radar and the pie)
/// is derived from the **two** per-channel bucket queries, so the heavy
/// aggregation runs a single `date_bin` scan per period per timeframe.
///
/// With `exclude_new_stations` (the Bike-Trends setting) only groups that have
/// measurements covering the whole current **and** previous window are folded
/// in (like-for-like), so newly-built stations no longer inflate the current
/// period. The check is generic over any window pair — a future custom from/to
/// date picker reuses it unchanged.
#[allow(clippy::too_many_arguments)]
pub(super) fn period_data(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: &Window,
    timezone: &str,
    tz: Tz,
    now: DateTime<Utc>,
    channel_ids: &[ChannelId],
    group_of_channel: &HashMap<uuid::Uuid, uuid::Uuid>,
    group_order: &[uuid::Uuid],
    exclude_new_stations: bool,
) -> Result<PeriodData, DomainError> {
    // Bike-Trends like-for-like filter: the set of groups that fully cover the
    // current AND the previous window, derived from per-channel coverage. Empty
    // (no filtering) when the setting is off.
    let established_groups: HashSet<uuid::Uuid> = if exclude_new_stations {
        // A still-running current window (`to >= now`, e.g. the current week or
        // year) never gates: its data may not have arrived yet, so every group
        // qualifies for it — only the completed previous window is decisive.
        let mut covers_current: HashSet<uuid::Uuid> = if current.to >= now {
            group_order.iter().copied().collect()
        } else {
            HashSet::new()
        };
        if current.to < now {
            let current_coverage =
                repository.resolution_coverage_by_channel(current.from, current.to, channel_ids)?;
            for row in &current_coverage {
                if covers_whole_window(
                    row.resolution_seconds,
                    row.first,
                    row.last,
                    current.from,
                    current.to,
                    now,
                ) && let Some(&group) = group_of_channel.get(&row.channel_id)
                {
                    covers_current.insert(group);
                }
            }
        }
        let mut covers_previous: HashSet<uuid::Uuid> = HashSet::new();
        let previous_coverage =
            repository.resolution_coverage_by_channel(previous.from, previous.to, channel_ids)?;
        for row in &previous_coverage {
            if covers_whole_window(
                row.resolution_seconds,
                row.first,
                row.last,
                previous.from,
                previous.to,
                now,
            ) && let Some(&group) = group_of_channel.get(&row.channel_id)
            {
                covers_previous.insert(group);
            }
        }
        covers_current
            .intersection(&covers_previous)
            .copied()
            .collect()
    } else {
        HashSet::new()
    };

    // Whether a channel belongs to an established (fully covered) group. With
    // the setting off every channel qualifies, so the default path is untouched.
    let in_established = |channel_id: uuid::Uuid| -> bool {
        !exclude_new_stations
            || group_of_channel
                .get(&channel_id)
                .is_some_and(|group| established_groups.contains(group))
    };

    let current_rows: Vec<ChannelBucket> = repository
        .sum_buckets_by_channel(
            current.from,
            current.to,
            current.bucket_seconds,
            current.origin,
            timezone,
            channel_ids,
            None,
        )?
        .into_iter()
        .filter(|row| in_established(row.channel_id))
        .collect();
    let previous_rows: Vec<ChannelBucket> = repository
        .sum_buckets_by_channel(
            previous.from,
            previous.to,
            previous.bucket_seconds,
            previous.origin,
            timezone,
            channel_ids,
            None,
        )?
        .into_iter()
        .filter(|row| in_established(row.channel_id))
        .collect();

    // Aggregate series: fold the per-channel buckets back into one series
    // keyed by bucket start (the repository orders by channel then bucket).
    let mut current_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
    for row in &current_rows {
        *current_map.entry(row.start).or_insert(0) += row.total;
    }
    let current_series: Vec<TimeBucket> = current_map
        .into_iter()
        .map(|(start, total)| TimeBucket { start, total })
        .collect();
    let mut previous_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
    for row in &previous_rows {
        *previous_map.entry(row.start).or_insert(0) += row.total;
    }
    let previous_series: Vec<TimeBucket> = previous_map
        .into_iter()
        .map(|(start, total)| TimeBucket { start, total })
        .collect();

    // Aggregate weekday radar: every bucket belongs to a single local
    // weekday, so folding the series yields the same totals as a per-row
    // `sum_weekdays`. Same for the previous period.
    let weekday_radar = weekday_totals(&current_series, tz);
    let weekday_radar_previous = weekday_totals(&previous_series, tz);

    // Per-group hour-of-day totals for the nerd-stats hour radar.
    let mut current_hour_by_group: HashMap<uuid::Uuid, Vec<HourTotal>> = HashMap::new();
    for row in
        repository.sum_hours_by_channel(current.from, current.to, timezone, channel_ids, None)?
    {
        if in_established(row.channel_id)
            && let Some(&group_id) = group_of_channel.get(&row.channel_id)
        {
            current_hour_by_group
                .entry(group_id)
                .or_default()
                .push(HourTotal {
                    hour: row.hour,
                    total: row.total,
                });
        }
    }
    let mut previous_hour_by_group: HashMap<uuid::Uuid, Vec<HourTotal>> = HashMap::new();
    for row in
        repository.sum_hours_by_channel(previous.from, previous.to, timezone, channel_ids, None)?
    {
        if in_established(row.channel_id)
            && let Some(&group_id) = group_of_channel.get(&row.channel_id)
        {
            previous_hour_by_group
                .entry(group_id)
                .or_default()
                .push(HourTotal {
                    hour: row.hour,
                    total: row.total,
                });
        }
    }

    // Aggregate hour-of-day radar over the raw measurements (the 30-day and
    // year buckets are 1-day wide and cannot be split into hours). With the
    // setting on the aggregate is folded from the (already filtered) per-group
    // rows, because the aggregate `sum_hours` cannot be restricted per group.
    let (hourly, hourly_previous) = if exclude_new_stations {
        let fold = |by_group: &HashMap<uuid::Uuid, Vec<HourTotal>>| {
            let mut totals: BTreeMap<u8, i64> = BTreeMap::new();
            for hours in by_group.values() {
                for hour in hours {
                    *totals.entry(hour.hour).or_insert(0) += hour.total;
                }
            }
            totals
                .into_iter()
                .map(|(hour, total)| HourTotal { hour, total })
                .collect()
        };
        (fold(&current_hour_by_group), fold(&previous_hour_by_group))
    } else {
        (
            repository.sum_hours(current.from, current.to, timezone, channel_ids, None)?,
            repository.sum_hours(previous.from, previous.to, timezone, channel_ids, None)?,
        )
    };

    // Per-group series + pie from the per-channel buckets.
    let mut current_by_group: HashMap<uuid::Uuid, Vec<TimeBucket>> = HashMap::new();
    let mut pie_by_group: HashMap<uuid::Uuid, i64> = HashMap::new();
    for row in current_rows {
        if let Some(&group_id) = group_of_channel.get(&row.channel_id) {
            current_by_group
                .entry(group_id)
                .or_default()
                .push(TimeBucket {
                    start: row.start,
                    total: row.total,
                });
            *pie_by_group.entry(group_id).or_insert(0) += row.total;
        }
    }
    let mut previous_by_group: HashMap<uuid::Uuid, Vec<TimeBucket>> = HashMap::new();
    for row in previous_rows {
        if let Some(&group_id) = group_of_channel.get(&row.channel_id) {
            previous_by_group
                .entry(group_id)
                .or_default()
                .push(TimeBucket {
                    start: row.start,
                    total: row.total,
                });
        }
    }

    // Keep the group order stable; a group is only included when it has
    // data in at least one of the two periods (non-established groups are
    // empty after filtering and are dropped automatically).
    let per_group = group_order
        .iter()
        .filter_map(|group_id| {
            let current = current_by_group.remove(group_id).unwrap_or_default();
            let previous = previous_by_group.remove(group_id).unwrap_or_default();
            if current.is_empty() && previous.is_empty() {
                return None;
            }
            Some(GroupGraphData {
                group_id: *group_id,
                weekday_radar: weekday_totals(&current, tz),
                weekday_radar_previous: weekday_totals(&previous, tz),
                hourly: current_hour_by_group.remove(group_id).unwrap_or_default(),
                hourly_previous: previous_hour_by_group.remove(group_id).unwrap_or_default(),
                pie_total: pie_by_group.get(group_id).copied().unwrap_or(0),
                current,
                previous,
            })
        })
        .collect();

    Ok(PeriodData {
        current: current_series,
        previous: previous_series,
        weekday_radar,
        weekday_radar_previous,
        hourly,
        hourly_previous,
        per_group,
    })
}

/// The per-channel detail graphs for one timeframe (nerd stats per channel).
///
/// The detail page is a single station, so nothing is filtered here. With
/// `exclude_new_stations` (the Bike-Trends setting) the station-level `is_new`
/// flag is derived from the station's coverage of the whole current + previous
/// window, so the UI can show a "no full-period data to compare" notice instead
/// of a misleading trend.
#[allow(clippy::too_many_arguments)]
pub(super) fn period_graphs_per_channel(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: &Window,
    timezone: &str,
    tz: Tz,
    now: DateTime<Utc>,
    channel_ids: &[ChannelId],
    channels: &[Channel],
    exclude_new_stations: bool,
) -> Result<PeriodGraphs, DomainError> {
    let group_of_channel: HashMap<uuid::Uuid, uuid::Uuid> =
        channels.iter().map(|c| (c.id.0, c.id.0)).collect();
    let group_order: Vec<uuid::Uuid> = channels.iter().map(|c| c.id.0).collect();
    let data = period_data(
        repository,
        current,
        previous,
        timezone,
        tz,
        now,
        channel_ids,
        &group_of_channel,
        &group_order,
        false,
    )?;

    // Station-level "new" flag (Bike-Trends): the station does not have
    // measurements covering the whole current or previous window, so there is no
    // like-for-like comparison. Computed over the station's channels in two
    // aggregate coverage queries.
    let is_new = if exclude_new_stations {
        let current_coverage =
            repository.resolution_coverage(current.from, current.to, channel_ids)?;
        let previous_coverage =
            repository.resolution_coverage(previous.from, previous.to, channel_ids)?;
        !has_full_coverage(&current_coverage, current.from, current.to, now)
            || !has_full_coverage(&previous_coverage, previous.from, previous.to, now)
    } else {
        false
    };

    let channel_pie = data
        .per_group
        .iter()
        .filter(|group| group.pie_total > 0)
        .map(|group| ChannelTotal {
            channel_id: group.group_id,
            total: group.pie_total,
        })
        .collect();
    let per_channel = data
        .per_group
        .into_iter()
        .map(|group| PerChannelSeries {
            channel_id: group.group_id,
            current: group.current,
            previous: group.previous,
            weekday_radar: group.weekday_radar,
            weekday_radar_previous: group.weekday_radar_previous,
            hourly: group.hourly,
            hourly_previous: group.hourly_previous,
        })
        .collect();

    Ok(PeriodGraphs {
        current: data.current,
        previous: data.previous,
        weekday_radar: data.weekday_radar,
        weekday_radar_previous: data.weekday_radar_previous,
        hourly: data.hourly,
        hourly_previous: data.hourly_previous,
        channel_pie,
        per_channel,
        is_new,
    })
}

/// The per-station summary graphs for one timeframe (nerd stats per station).
/// With `exclude_new_stations` only stations with full coverage of the current
/// AND previous window are aggregated (like-for-like).
#[allow(clippy::too_many_arguments)]
pub(super) fn period_graphs_per_station(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: &Window,
    timezone: &str,
    tz: Tz,
    now: DateTime<Utc>,
    channel_ids: &[ChannelId],
    station_ids: &[uuid::Uuid],
    station_of_channel: &HashMap<uuid::Uuid, uuid::Uuid>,
    exclude_new_stations: bool,
) -> Result<SummaryPeriodGraphs, DomainError> {
    let data = period_data(
        repository,
        current,
        previous,
        timezone,
        tz,
        now,
        channel_ids,
        station_of_channel,
        station_ids,
        exclude_new_stations,
    )?;

    let station_pie = data
        .per_group
        .iter()
        .filter(|group| group.pie_total > 0)
        .map(|group| StationTotal {
            station_id: group.group_id,
            total: group.pie_total,
        })
        .collect();
    let per_station = data
        .per_group
        .into_iter()
        .map(|group| PerStationSeries {
            station_id: group.group_id,
            current: group.current,
            previous: group.previous,
            weekday_radar: group.weekday_radar,
            weekday_radar_previous: group.weekday_radar_previous,
            hourly: group.hourly,
            hourly_previous: group.hourly_previous,
        })
        .collect();

    Ok(SummaryPeriodGraphs {
        current: data.current,
        previous: data.previous,
        weekday_radar: data.weekday_radar,
        weekday_radar_previous: data.weekday_radar_previous,
        hourly: data.hourly,
        hourly_previous: data.hourly_previous,
        station_pie,
        per_station,
    })
}
