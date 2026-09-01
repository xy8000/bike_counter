//! Application helpers that bucket measurement sums into the detail/summary
//! time-series graphs (day / week / last 30 days / year), the weekday and hour
//! radars and the pie charts.
//!
//! All window math lives here (see [`graph_windows`]); this is the future seam
//! for a date-picker: swapping the `now`-derived "previous complete period"
//! windows for an arbitrary selected range touches only this module (and
//! [`super::metrics`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use chrono_tz::Tz;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::{
    local_days_window, local_week_start, local_year_start, previous_calendar_year,
    previous_local_days,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{
    BucketGranularity, ChannelBucket, ChannelTotal, HourTotal, MeasurementRepository, TimeBucket,
    WeekdayTotal,
};
use crate::core::domain::station_analytics::{
    PerChannelSeries, PerStationSeries, PeriodGraphs, StationTotal, SummaryPeriodGraphs,
};

use super::resolution::introduced_after;

/// Fixed bucket widths (seconds) used by the detail/summary graphs.
const SECONDS_PER_5_MINUTES: i64 = 5 * 60;
const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// One time-window: `[from, to]` with the bucket granularity and alignment
/// origin (used by the fixed-width `date_bin` buckets; calendar granularities
/// ignore it).
pub(super) struct Window {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    granularity: BucketGranularity,
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
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_5_MINUTES,
                },
                origin: day_from,
            },
            previous: Window {
                from: previous_day_from,
                to: previous_day_to,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_5_MINUTES,
                },
                origin: previous_day_from,
            },
        },
        week: WindowPair {
            current: Window {
                from: week_start,
                to: now,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_HOUR,
                },
                origin: week_start,
            },
            previous: Window {
                from: last_week_from,
                to: last_week_to,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_HOUR,
                },
                origin: last_week_from,
            },
        },
        last_30_days: WindowPair {
            current: Window {
                from: last_30_from,
                to: last_30_to,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_DAY,
                },
                origin: last_30_from,
            },
            previous: Window {
                from: previous_30_from,
                to: previous_30_to,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_DAY,
                },
                origin: previous_30_from,
            },
        },
        year: WindowPair {
            current: Window {
                from: year_start,
                to: now,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_DAY,
                },
                origin: year_start,
            },
            previous: Window {
                from: last_year_from,
                to: last_year_to,
                granularity: BucketGranularity::Fixed {
                    seconds: SECONDS_PER_DAY,
                },
                origin: last_year_from,
            },
        },
    })
}

/// Whether buckets of this granularity are at most one day wide, so folding them
/// into weekday totals is meaningful.
fn is_daily_or_finer(granularity: BucketGranularity) -> bool {
    match granularity {
        BucketGranularity::Fixed { seconds } => seconds <= SECONDS_PER_DAY,
        BucketGranularity::Day => true,
        BucketGranularity::Week | BucketGranularity::Month | BucketGranularity::Quarter => false,
    }
}

/// The calendar-aligned bucket granularity for a custom from/to range, chosen
/// by the range length: `<= 24h` → 15 minutes, `<= 48h` → 1 hour, `<= 30d` →
/// 1 day, `<= 90d` → 1 week, `<= 2y` → 1 month, otherwise → 1 quarter.
fn custom_granularity(span: Duration) -> BucketGranularity {
    let hours = span.num_hours();
    let days = span.num_days();
    if hours <= 24 {
        BucketGranularity::Fixed { seconds: 15 * 60 }
    } else if hours <= 48 {
        BucketGranularity::Fixed { seconds: 3600 }
    } else if days <= 30 {
        BucketGranularity::Day
    } else if days <= 90 {
        BucketGranularity::Week
    } else if days <= 730 {
        BucketGranularity::Month
    } else {
        BucketGranularity::Quarter
    }
}

/// A single custom window `[from, to]` for the "Individual" date range, with
/// the granularity derived from the range length. There is no previous period —
/// the compare checkbox is disabled for custom ranges. Fixed-width hour /
/// 15-minute buckets are aligned to the local midnight of `from` so they sit on
/// quarter-hour / hour boundaries.
pub(super) fn custom_window(
    tz: Tz,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Window, DomainError> {
    if from >= to {
        return Err(DomainError::InvalidQuery(
            "from must be before to".to_string(),
        ));
    }
    let granularity = custom_granularity(to - from);
    let origin = match granularity {
        BucketGranularity::Fixed { .. } => {
            let local = from.with_timezone(&tz);
            let midnight = local
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| DomainError::InvalidQuery("invalid from date".to_string()))?;
            tz.from_local_datetime(&midnight)
                .earliest()
                .ok_or_else(|| DomainError::InvalidQuery("invalid from date".to_string()))?
                .with_timezone(&Utc)
        }
        _ => from,
    };
    Ok(Window {
        from,
        to,
        granularity,
        origin,
    })
}

/// The UTC start of the first calendar bucket (`day`/`week`/`month`/`quarter`)
/// containing `time`, in `tz` — mirrors the repository's `date_trunc`.
fn calendar_bucket_start(
    time: DateTime<Utc>,
    tz: Tz,
    granularity: BucketGranularity,
) -> DateTime<Utc> {
    let local = time.with_timezone(&tz);
    let date = local.date_naive();
    let midnight = match granularity {
        BucketGranularity::Day => date,
        BucketGranularity::Week => {
            date - chrono::Days::new(local.weekday().num_days_from_monday() as u64)
        }
        BucketGranularity::Month => chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
            .expect("valid month bucket"),
        BucketGranularity::Quarter => {
            let month = ((date.month() - 1) / 3) * 3 + 1;
            chrono::NaiveDate::from_ymd_opt(date.year(), month, 1).expect("valid quarter bucket")
        }
        BucketGranularity::Fixed { .. } => unreachable!("fixed granularity has no calendar bucket"),
    };
    let naive = midnight.and_hms_opt(0, 0, 0).expect("local midnight");
    tz.from_local_datetime(&naive)
        .earliest()
        .expect("valid local midnight")
        .with_timezone(&Utc)
}

/// Advances `time` by `months` local calendar months (used for the month and
/// quarter bucket steps, whose lengths vary).
fn add_local_months(time: DateTime<Utc>, tz: Tz, months: u32) -> DateTime<Utc> {
    time.with_timezone(&tz)
        .checked_add_months(chrono::Months::new(months))
        .map(|next| next.with_timezone(&Utc))
        .unwrap_or(time)
}

/// The complete list of bucket starts of a granularity across `[from, to]`,
/// aligned like the repository's `date_bin`/`date_trunc`. Used to zero-fill a
/// custom date range so the whole selected period is drawn (months/weeks
/// without traffic render as 0 instead of being omitted).
fn bucket_starts(
    granularity: BucketGranularity,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    origin: DateTime<Utc>,
    tz: Tz,
) -> Vec<DateTime<Utc>> {
    let mut starts = Vec::new();
    // Half-open grid over `[from, to)`: a bucket starting exactly at `to` is the
    // first bucket outside the selection, so it is not drawn as an empty bar.
    match granularity {
        BucketGranularity::Fixed { seconds } => {
            let elapsed = (from - origin).num_seconds().div_euclid(seconds);
            let mut cur = origin + Duration::seconds(elapsed * seconds);
            while cur < to {
                starts.push(cur);
                cur += Duration::seconds(seconds);
            }
        }
        granularity @ (BucketGranularity::Day
        | BucketGranularity::Week
        | BucketGranularity::Month
        | BucketGranularity::Quarter) => {
            let step_months = match granularity {
                BucketGranularity::Month => Some(1),
                BucketGranularity::Quarter => Some(3),
                _ => None,
            };
            let mut cur = calendar_bucket_start(from, tz, granularity);
            while cur < to {
                starts.push(cur);
                cur = match step_months {
                    Some(months) => add_local_months(cur, tz, months),
                    None => {
                        cur + match granularity {
                            BucketGranularity::Day => Duration::days(1),
                            BucketGranularity::Week => Duration::days(7),
                            _ => unreachable!(),
                        }
                    }
                };
            }
        }
    }
    starts
}

/// Zero-fills a bucket series across `[from, to]` at the window's granularity,
/// so a custom date range always draws the whole selected period.
fn zero_fill_buckets(
    series: Vec<TimeBucket>,
    granularity: BucketGranularity,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    origin: DateTime<Utc>,
    tz: Tz,
) -> Vec<TimeBucket> {
    let totals: BTreeMap<DateTime<Utc>, i64> = series
        .into_iter()
        .map(|bucket| (bucket.start, bucket.total))
        .collect();
    bucket_starts(granularity, from, to, origin, tz)
        .into_iter()
        .map(|start| TimeBucket {
            start,
            total: totals.get(&start).copied().unwrap_or(0),
        })
        .collect()
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

/// Folds per-group weekday totals into an aggregate. Used when the buckets are
/// wider than a day (custom week/month/quarter ranges) and the weekday radar
/// must come from raw measurements instead of the bucket series.
fn fold_weekdays(by_group: &HashMap<uuid::Uuid, Vec<WeekdayTotal>>) -> Vec<WeekdayTotal> {
    let mut totals: BTreeMap<u8, i64> = BTreeMap::new();
    for weekdays in by_group.values() {
        for weekday in weekdays {
            *totals.entry(weekday.weekday).or_insert(0) += weekday.total;
        }
    }
    totals
        .into_iter()
        .filter(|(_, total)| *total > 0)
        .map(|(weekday, total)| WeekdayTotal { weekday, total })
        .collect()
}

/// The aggregate + per-group data for one window pair. A "group" is a channel
/// (detail page) or a station (summary page); `group_of_channel` maps each
/// channel id to its group id and `group_order` gives the stable output
/// order. Everything (the aggregate series, the weekday radar and the pie)
/// is derived from the **two** per-channel bucket queries, so the heavy
/// aggregation runs a single bucket scan per period.
///
/// With `exclude_new_stations` (the Bike-Trends setting) only groups that
/// already existed before the comparison window are folded in (like-for-like) —
/// a group is dropped only when it was introduced at or after the window's
/// start, so data loss or outages inside the window never exclude it. A custom
/// from/to range has no previous period (`previous = None`): the current
/// window's start is the reference, and the previous outputs stay empty.
#[allow(clippy::too_many_arguments)]
pub(super) fn period_data(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: Option<&Window>,
    timezone: &str,
    tz: Tz,
    _now: DateTime<Utc>,
    channel_ids: &[ChannelId],
    group_of_channel: &HashMap<uuid::Uuid, uuid::Uuid>,
    group_order: &[uuid::Uuid],
    exclude_new_stations: bool,
) -> Result<PeriodData, DomainError> {
    // Bike-Trends like-for-like filter: the set of groups that already existed
    // before the comparison window started — their earliest-ever measurement is
    // before `reference_from` (the previous window's start, or the current
    // window's start for a custom range with no previous period). A group
    // introduced during the window (no data before its start) is dropped; data
    // loss or outages inside the window never exclude a group. Empty (no
    // filtering) when the setting is off.
    let established_groups: HashSet<uuid::Uuid> = if exclude_new_stations {
        let reference_from = previous
            .as_ref()
            .map_or(current.from, |previous| previous.from);
        let mut earliest_by_group: HashMap<uuid::Uuid, DateTime<Utc>> = HashMap::new();
        for row in repository.earliest_by_channel(channel_ids)? {
            if let Some(&group) = group_of_channel.get(&row.channel_id) {
                let entry = earliest_by_group.entry(group).or_insert(row.timestamp);
                *entry = (*entry).min(row.timestamp);
            }
        }
        earliest_by_group
            .into_iter()
            .filter(|(_, earliest)| !introduced_after(*earliest, reference_from))
            .map(|(group, _)| group)
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
            current.granularity,
            current.origin,
            timezone,
            channel_ids,
            None,
        )?
        .into_iter()
        .filter(|row| in_established(row.channel_id))
        .collect();
    let previous_rows: Vec<ChannelBucket> = previous
        .map(|previous| {
            repository.sum_buckets_by_channel(
                previous.from,
                previous.to,
                previous.granularity,
                previous.origin,
                timezone,
                channel_ids,
                None,
            )
        })
        .transpose()?
        .unwrap_or_default()
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
    // A custom range has no previous period and the user picked the exact
    // from/to, so the aggregate series is zero-filled to draw the whole period
    // (months/weeks without traffic render as 0 instead of being omitted).
    // When the range has no data at all the series stays empty, so the UI can
    // still show an empty state instead of a full grid of zero bars.
    let current_series = if previous.is_none() && !current_series.is_empty() {
        zero_fill_buckets(
            current_series,
            current.granularity,
            current.from,
            current.to,
            current.origin,
            tz,
        )
    } else {
        current_series
    };
    let mut previous_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
    for row in &previous_rows {
        *previous_map.entry(row.start).or_insert(0) += row.total;
    }
    let previous_series: Vec<TimeBucket> = previous_map
        .into_iter()
        .map(|(start, total)| TimeBucket { start, total })
        .collect();

    // Weekday radar: with daily-or-finer buckets (the fixed timeframes) folding
    // the bucket series yields the same totals as a per-row sum. With wider
    // custom-range buckets (week/month/quarter) the weekday radar is computed
    // over the raw measurements via `sum_weekdays_by_channel`, restricted to
    // established groups. A custom range has no previous period, so the
    // previous weekday radar stays empty.
    let wide_buckets = !is_daily_or_finer(current.granularity);
    let mut current_weekday_by_group: HashMap<uuid::Uuid, Vec<WeekdayTotal>> = HashMap::new();
    if wide_buckets {
        for row in repository.sum_weekdays_by_channel(
            current.from,
            current.to,
            timezone,
            channel_ids,
            None,
        )? {
            if in_established(row.channel_id)
                && let Some(&group_id) = group_of_channel.get(&row.channel_id)
            {
                current_weekday_by_group
                    .entry(group_id)
                    .or_default()
                    .push(WeekdayTotal {
                        weekday: row.weekday,
                        total: row.total,
                    });
            }
        }
    }
    let weekday_radar = if wide_buckets {
        fold_weekdays(&current_weekday_by_group)
    } else {
        weekday_totals(&current_series, tz)
    };
    let weekday_radar_previous = if wide_buckets {
        Vec::new()
    } else {
        weekday_totals(&previous_series, tz)
    };

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
    if let Some(previous) = previous {
        for row in repository.sum_hours_by_channel(
            previous.from,
            previous.to,
            timezone,
            channel_ids,
            None,
        )? {
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
        let previous_hours = match previous {
            Some(previous) => {
                repository.sum_hours(previous.from, previous.to, timezone, channel_ids, None)?
            }
            None => Vec::new(),
        };
        (
            repository.sum_hours(current.from, current.to, timezone, channel_ids, None)?,
            previous_hours,
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
    // empty after filtering and are dropped automatically). For a custom
    // range the per-group current series is zero-filled too, so the
    // per-channel / per-station nerd charts draw the whole selected period.
    let custom_range = previous.is_none();
    let per_group = group_order
        .iter()
        .filter_map(|group_id| {
            let mut current_series = current_by_group.remove(group_id).unwrap_or_default();
            let previous_series = previous_by_group.remove(group_id).unwrap_or_default();
            if current_series.is_empty() && previous_series.is_empty() {
                return None;
            }
            if custom_range && !current_series.is_empty() {
                current_series = zero_fill_buckets(
                    current_series,
                    current.granularity,
                    current.from,
                    current.to,
                    current.origin,
                    tz,
                );
            }
            let (weekday_radar, weekday_radar_previous) = if wide_buckets {
                (
                    current_weekday_by_group
                        .remove(group_id)
                        .unwrap_or_default(),
                    Vec::new(),
                )
            } else {
                (
                    weekday_totals(&current_series, tz),
                    weekday_totals(&previous_series, tz),
                )
            };
            Some(GroupGraphData {
                group_id: *group_id,
                weekday_radar,
                weekday_radar_previous,
                hourly: current_hour_by_group.remove(group_id).unwrap_or_default(),
                hourly_previous: previous_hour_by_group.remove(group_id).unwrap_or_default(),
                pie_total: pie_by_group.get(group_id).copied().unwrap_or(0),
                current: current_series,
                previous: previous_series,
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
/// flag is set when the station was introduced at or after the comparison
/// window's start, so the UI can show a "opened during the period" notice
/// instead of a misleading trend.
#[allow(clippy::too_many_arguments)]
pub(super) fn period_graphs_per_channel(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: Option<&Window>,
    timezone: &str,
    tz: Tz,
    _now: DateTime<Utc>,
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
        _now,
        channel_ids,
        &group_of_channel,
        &group_order,
        false,
    )?;

    // Station-level "new" flag (Bike-Trends): the station was introduced at or
    // after the comparison window's start (its earliest-ever measurement is not
    // before the previous window's start, or the current window's start for a
    // custom range), so there is no like-for-like baseline. Computed over the
    // station's channels in one aggregate earliest query. Data loss or outages
    // inside the window never flag the station as new.
    let is_new = if exclude_new_stations {
        let reference_from = previous
            .as_ref()
            .map_or(current.from, |previous| previous.from);
        match repository
            .earliest_by_channel(channel_ids)?
            .into_iter()
            .map(|channel_first| channel_first.timestamp)
            .min()
        {
            // Introduced inside the window, or no data at all -> new.
            Some(earliest) => introduced_after(earliest, reference_from),
            None => true,
        }
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
/// With `exclude_new_stations` only stations that already existed before the
/// comparison window are aggregated (like-for-like).
#[allow(clippy::too_many_arguments)]
pub(super) fn period_graphs_per_station(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: Option<&Window>,
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
    use chrono_tz::Tz;

    use super::*;
    use crate::core::domain::measurements::repository_port::BucketGranularity;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    fn berlin() -> Tz {
        "Europe/Berlin".parse().unwrap()
    }

    fn bucket(start: DateTime<Utc>, total: i64) -> TimeBucket {
        TimeBucket { start, total }
    }

    #[test]
    fn custom_granularity_selects_each_tier_and_boundaries() {
        // `<= 24h` -> 15 minutes.
        assert_eq!(
            custom_granularity(Duration::hours(24)),
            BucketGranularity::Fixed { seconds: 15 * 60 }
        );
        assert_eq!(
            custom_granularity(Duration::hours(25)),
            BucketGranularity::Fixed { seconds: 3600 }
        );
        // `<= 48h` -> 1 hour.
        assert_eq!(
            custom_granularity(Duration::hours(48)),
            BucketGranularity::Fixed { seconds: 3600 }
        );
        // `<= 30d` -> 1 day.
        assert_eq!(
            custom_granularity(Duration::hours(49)),
            BucketGranularity::Day
        );
        assert_eq!(
            custom_granularity(Duration::days(30)),
            BucketGranularity::Day
        );
        // `<= 90d` -> 1 week.
        assert_eq!(
            custom_granularity(Duration::days(31)),
            BucketGranularity::Week
        );
        assert_eq!(
            custom_granularity(Duration::days(90)),
            BucketGranularity::Week
        );
        // `<= 2y` (730d) -> 1 month.
        assert_eq!(
            custom_granularity(Duration::days(91)),
            BucketGranularity::Month
        );
        assert_eq!(
            custom_granularity(Duration::days(730)),
            BucketGranularity::Month
        );
        // `> 2y` -> 1 quarter.
        assert_eq!(
            custom_granularity(Duration::days(731)),
            BucketGranularity::Quarter
        );
    }

    #[test]
    fn is_daily_or_finer_flags_only_daily_and_finer_buckets() {
        assert!(is_daily_or_finer(BucketGranularity::Fixed {
            seconds: 5 * 60
        }));
        assert!(is_daily_or_finer(BucketGranularity::Fixed {
            seconds: 60 * 60
        }));
        assert!(is_daily_or_finer(BucketGranularity::Fixed {
            seconds: 24 * 60 * 60
        }));
        assert!(!is_daily_or_finer(BucketGranularity::Fixed {
            seconds: 48 * 60 * 60
        }));
        assert!(is_daily_or_finer(BucketGranularity::Day));
        assert!(!is_daily_or_finer(BucketGranularity::Week));
        assert!(!is_daily_or_finer(BucketGranularity::Month));
        assert!(!is_daily_or_finer(BucketGranularity::Quarter));
    }

    #[test]
    fn calendar_bucket_start_anchors_each_granularity_in_the_local_timezone() {
        // 2024-01-10 13:00 Berlin (CET, UTC+1).
        let time = utc(2024, 1, 10, 12, 0, 0);
        assert_eq!(
            calendar_bucket_start(time, berlin(), BucketGranularity::Day),
            utc(2024, 1, 9, 23, 0, 0)
        );
        // Wednesday -> the local Monday (Jan 8).
        assert_eq!(
            calendar_bucket_start(time, berlin(), BucketGranularity::Week),
            utc(2024, 1, 7, 23, 0, 0)
        );
        // Month -> Jan 1 00:00 Berlin.
        assert_eq!(
            calendar_bucket_start(time, berlin(), BucketGranularity::Month),
            utc(2023, 12, 31, 23, 0, 0)
        );
        // Quarter (Q1) -> Jan 1 00:00 Berlin, same as the month for January.
        assert_eq!(
            calendar_bucket_start(time, berlin(), BucketGranularity::Quarter),
            utc(2023, 12, 31, 23, 0, 0)
        );
        // A mid-quarter date anchors to the quarter's first month.
        let april = utc(2024, 4, 15, 12, 0, 0);
        assert_eq!(
            calendar_bucket_start(april, berlin(), BucketGranularity::Quarter),
            utc(2024, 3, 31, 22, 0, 0)
        );
    }

    #[test]
    fn add_local_months_preserves_the_local_clock_across_a_dst_transition() {
        // 2024-09-30 14:00 Berlin CEST, one month later is 2024-10-30 14:00 CET:
        // the local clock time survives the CEST -> CET switch (Oct 27, 2024).
        let start = utc(2024, 9, 30, 12, 0, 0);
        let next = add_local_months(start, berlin(), 1).with_timezone(&berlin());
        assert_eq!(
            (next.month(), next.day(), next.hour(), next.minute()),
            (10, 30, 14, 0)
        );
    }

    #[test]
    fn add_local_months_clamps_to_the_last_day_of_a_shorter_month() {
        // 2024-01-31 + 1 local month -> 2024-02-29 (leap year).
        let start = utc(2024, 1, 31, 12, 0, 0);
        let next = add_local_months(start, berlin(), 1).with_timezone(&berlin());
        assert_eq!((next.month(), next.day()), (2, 29));
    }

    #[test]
    fn bucket_starts_fixed_grid_is_half_open_and_stops_before_to() {
        let from = utc(2024, 1, 10, 0, 0, 0);
        let to = utc(2024, 1, 10, 3, 0, 0);
        let starts = bucket_starts(
            BucketGranularity::Fixed { seconds: 3600 },
            from,
            to,
            from,
            berlin(),
        );
        assert_eq!(
            starts,
            vec![
                utc(2024, 1, 10, 0, 0, 0),
                utc(2024, 1, 10, 1, 0, 0),
                utc(2024, 1, 10, 2, 0, 0),
            ]
        );
    }

    #[test]
    fn bucket_starts_month_grid_steps_local_calendar_months() {
        // Jan 10 2024 .. Mar 1 2024 (Berlin): the local-month buckets are
        // Jan 1, Feb 1 and Mar 1 00:00 Berlin.
        let from = utc(2024, 1, 10, 0, 0, 0);
        let to = utc(2024, 3, 1, 0, 0, 0);
        let starts = bucket_starts(BucketGranularity::Month, from, to, from, berlin());
        assert_eq!(
            starts,
            vec![
                utc(2023, 12, 31, 23, 0, 0),
                utc(2024, 1, 31, 23, 0, 0),
                utc(2024, 2, 29, 23, 0, 0),
            ]
        );
    }

    #[test]
    fn bucket_starts_quarter_grid_steps_three_local_months() {
        // Jan 10 2024 .. Jul 1 2024 (Berlin): Q1 (Jan 1), Q2 (Apr 1) and
        // Q3 (Jul 1) local-quarter starts.
        let from = utc(2024, 1, 10, 0, 0, 0);
        let to = utc(2024, 7, 1, 0, 0, 0);
        let starts = bucket_starts(BucketGranularity::Quarter, from, to, from, berlin());
        assert_eq!(
            starts,
            vec![
                utc(2023, 12, 31, 23, 0, 0),
                utc(2024, 3, 31, 22, 0, 0),
                utc(2024, 6, 30, 22, 0, 0),
            ]
        );
    }

    #[test]
    fn zero_fill_buckets_keeps_totals_and_fills_gaps_with_zero() {
        let series = vec![bucket(utc(2024, 1, 10, 1, 0, 0), 5)];
        let from = utc(2024, 1, 10, 0, 0, 0);
        let to = utc(2024, 1, 10, 3, 0, 0);
        let filled = zero_fill_buckets(
            series,
            BucketGranularity::Fixed { seconds: 3600 },
            from,
            to,
            from,
            berlin(),
        );
        assert_eq!(
            filled,
            vec![
                bucket(utc(2024, 1, 10, 0, 0, 0), 0),
                bucket(utc(2024, 1, 10, 1, 0, 0), 5),
                bucket(utc(2024, 1, 10, 2, 0, 0), 0),
            ]
        );
    }

    #[test]
    fn zero_fill_buckets_empty_series_fills_the_whole_window() {
        let from = utc(2024, 1, 10, 0, 0, 0);
        let to = utc(2024, 1, 10, 2, 0, 0);
        let filled = zero_fill_buckets(
            Vec::new(),
            BucketGranularity::Fixed { seconds: 3600 },
            from,
            to,
            from,
            berlin(),
        );
        assert_eq!(
            filled,
            vec![
                bucket(utc(2024, 1, 10, 0, 0, 0), 0),
                bucket(utc(2024, 1, 10, 1, 0, 0), 0),
            ]
        );
    }

    #[test]
    fn weekday_totals_sums_by_local_weekday_and_omits_quiet_weekdays() {
        // Jan 8 2024 is a Monday, Jan 12 2024 is a Friday (in Berlin).
        let buckets = vec![
            bucket(utc(2024, 1, 8, 0, 0, 0), 10),
            bucket(utc(2024, 1, 8, 1, 0, 0), 5),
            bucket(utc(2024, 1, 12, 0, 0, 0), 7),
        ];
        assert_eq!(
            weekday_totals(&buckets, berlin()),
            vec![
                WeekdayTotal {
                    weekday: 1,
                    total: 15
                },
                WeekdayTotal {
                    weekday: 5,
                    total: 7
                },
            ]
        );
    }

    #[test]
    fn weekday_totals_empty_series_is_empty() {
        assert_eq!(weekday_totals(&[], berlin()), Vec::new());
    }

    #[test]
    fn fold_weekdays_aggregates_across_groups_and_drops_zeroes() {
        let mut by_group = HashMap::new();
        by_group.insert(
            uuid::Uuid::from_u128(0x1),
            vec![
                WeekdayTotal {
                    weekday: 1,
                    total: 10,
                },
                WeekdayTotal {
                    weekday: 5,
                    total: 7,
                },
            ],
        );
        by_group.insert(
            uuid::Uuid::from_u128(0x2),
            vec![
                WeekdayTotal {
                    weekday: 1,
                    total: 3,
                },
                WeekdayTotal {
                    weekday: 3,
                    total: 2,
                },
            ],
        );
        assert_eq!(
            fold_weekdays(&by_group),
            vec![
                WeekdayTotal {
                    weekday: 1,
                    total: 13
                },
                WeekdayTotal {
                    weekday: 3,
                    total: 2
                },
                WeekdayTotal {
                    weekday: 5,
                    total: 7
                },
            ]
        );
    }

    #[test]
    fn fold_weekdays_empty_input_is_empty() {
        assert_eq!(fold_weekdays(&HashMap::new()), Vec::new());
    }
}
