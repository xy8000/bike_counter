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

use super::resolution::{covers_whole_window, has_full_coverage};

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
/// With `exclude_new_stations` (the Bike-Trends setting) only groups that have
/// measurements covering the whole current **and** previous window are folded
/// in (like-for-like), so newly-built stations no longer inflate the current
/// period. A custom from/to range has no previous period (`previous = None`):
/// only the current window gates, and the previous outputs stay empty.
#[allow(clippy::too_many_arguments)]
pub(super) fn period_data(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: Option<&Window>,
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
        let covers_previous: HashSet<uuid::Uuid> = match previous {
            Some(previous) => {
                let mut covers = HashSet::new();
                let previous_coverage = repository.resolution_coverage_by_channel(
                    previous.from,
                    previous.to,
                    channel_ids,
                )?;
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
                        covers.insert(group);
                    }
                }
                covers
            }
            // No previous period (custom range): only the current window gates.
            None => group_order.iter().copied().collect(),
        };
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
/// flag is derived from the station's coverage of the whole current + previous
/// window, so the UI can show a "no full-period data to compare" notice instead
/// of a misleading trend.
#[allow(clippy::too_many_arguments)]
pub(super) fn period_graphs_per_channel(
    repository: &dyn MeasurementRepository,
    current: &Window,
    previous: Option<&Window>,
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
    // like-for-like comparison. Computed over the station's channels in
    // aggregate coverage queries. A custom range has no previous period, so
    // only the current window is checked.
    let is_new = if exclude_new_stations {
        let current_coverage =
            repository.resolution_coverage(current.from, current.to, channel_ids)?;
        let current_ok = has_full_coverage(&current_coverage, current.from, current.to, now);
        let previous_ok = match previous {
            Some(previous) => {
                let previous_coverage =
                    repository.resolution_coverage(previous.from, previous.to, channel_ids)?;
                has_full_coverage(&previous_coverage, previous.from, previous.to, now)
            }
            None => true,
        };
        !current_ok || !previous_ok
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
