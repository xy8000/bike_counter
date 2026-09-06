/// Shared timeframe presentation for the detail page and the station-summary
/// page: the selectable timeframes, the axis/tooltip formatters and the
/// period-alignment helpers. Both pages render the same charts, so the config
/// lives here instead of being duplicated.

import { formatFullDate, formatFullDateTime, LOCALE } from '../../lib/format'
import type { FixedTimeframe, TimeBucket, Timeframe } from './types'
import type { BarSeries } from './TimeSeriesBarChart'

type TimeUnit = 'hour' | 'day' | 'month'

/// Axis label for a bucket timestamp, tuned to the window's density. Labels use
/// the central `LOCALE` so a future locale change needs no per-chart edits.
function timeAxis(unit: TimeUnit): (time: number) => string {
  switch (unit) {
    case 'hour':
      return (time) =>
        new Date(time).toLocaleTimeString(LOCALE, { hour: '2-digit', minute: '2-digit' })
    case 'day':
      return (time) =>
        new Date(time).toLocaleDateString(LOCALE, { day: '2-digit', month: '2-digit' })
    case 'month':
      return (time) => new Date(time).toLocaleDateString(LOCALE, { month: 'short' })
  }
}

/// Axis label for sub-day buckets spanning several days: `dd.MM., HH:mm`. A bare
/// `HH:mm` repeats on every day (all ticks collapse to `00:00`), so the day is
/// prepended once a window crosses more than one day.
function dayTimeAxis(time: number): string {
  const date = new Date(time)
  const day = date.toLocaleDateString(LOCALE, { day: '2-digit', month: '2-digit' })
  const timeOfDay = date.toLocaleTimeString(LOCALE, { hour: '2-digit', minute: '2-digit' })
  return `${day}, ${timeOfDay}`
}

/// Axis label with a 2-digit year (`dd.MM.yy`), used when day/week-granularity
/// buckets can cross a year boundary and a bare `dd.MM.` would be ambiguous.
function dayYearAxis(time: number): string {
  return new Date(time).toLocaleDateString(LOCALE, {
    day: '2-digit',
    month: '2-digit',
    year: '2-digit',
  })
}

/// Wraps an axis formatter so consecutive duplicate labels collapse to a single
/// tick (e.g. the year view: several day/week buckets fall in the same month and
/// would repeat `Jan Jan Jan …`). Each config builds its own closure per render,
/// so the per-tick sequence resets cleanly between renders.
function dedupe(axis: (time: number) => string): (time: number) => string {
  let previous = ''
  return (time) => {
    const label = axis(time)
    if (label === previous) return ''
    previous = label
    return label
  }
}

/// Axis label for the overlapped week chart: the aligned domain is anchored on a
/// Monday, so a short weekday name marks each day. The week timeframe uses
/// 1-hour buckets, so the local time is appended — a bare weekday name would
/// repeat (e.g. `Mo Mo Mo Mo`) when Recharts emits several ticks per day.
function weekdayAxis(time: number): string {
  const date = new Date(time)
  const weekday = date.toLocaleDateString(LOCALE, { weekday: 'short' })
  const timeOfDay = date.toLocaleTimeString(LOCALE, { hour: '2-digit', minute: '2-digit' })
  return `${weekday} ${timeOfDay}`
}

/// Browser-local midnight of the day containing `time` (period start for the day
/// and 30-day timeframes).
function dayStartOf(time: number): number {
  const date = new Date(time)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

/// Browser-local midnight of the Monday of the ISO week containing `time`.
function weekStartOf(time: number): number {
  const date = new Date(time)
  const sinceMonday = (date.getDay() + 6) % 7
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() - sinceMonday).getTime()
}

/// Browser-local midnight of Jan 1 of the year containing `time`.
function yearStartOf(time: number): number {
  const date = new Date(time)
  return new Date(date.getFullYear(), 0, 1).getTime()
}

/// Per-timeframe presentation config: labels, axis/tooltip formatters, the
/// period-start function used for the overlap alignment and the fixed axis
/// width. The year timeframe computes its domain to the actual next Jan 1.
export interface TimeframeConfig {
  key: Timeframe
  label: string
  title: string
  subtitle: string
  radarSubtitle: string
  pieSubtitle: string
  perChannelTitle: string
  currentLabel: string
  previousLabel: string
  axis: (time: number) => string
  /** True when this view's x-axis labels are long and dense (e.g. `dd.MM.,
   *  HH:mm` across 30 days), so the chart rotates the ticks -45° to fit. */
  axisRotate?: boolean
  tooltip: (time: number) => string
  periodStart: (time: number) => number
}

export const TIMEFRAMES: Record<FixedTimeframe, TimeframeConfig> = {
  day: {
    key: 'day',
    label: '24 hours',
    title: '24 hours',
    subtitle: '5-minute buckets',
    radarSubtitle: 'the last complete day',
    pieSubtitle: 'over the last day',
    perChannelTitle: '24 hours by channel',
    currentLabel: 'Last day',
    previousLabel: 'Day before',
    axis: timeAxis('hour'),
    tooltip: formatFullDateTime,
    periodStart: dayStartOf,
  },
  week: {
    key: 'week',
    label: 'This week',
    title: 'This week',
    subtitle: '1-hour buckets',
    radarSubtitle: 'the current week',
    pieSubtitle: 'over the current week',
    perChannelTitle: 'This week by channel',
    currentLabel: 'Current week',
    previousLabel: 'Last week',
    axis: weekdayAxis,
    tooltip: formatFullDateTime,
    periodStart: weekStartOf,
  },
  last_30_days: {
    key: 'last_30_days',
    label: 'Last 30 days',
    title: 'Last 30 days',
    subtitle: '1-day buckets',
    radarSubtitle: 'the last 30 days',
    pieSubtitle: 'over the last 30 days',
    perChannelTitle: 'Last 30 days by channel',
    currentLabel: 'Last 30 days',
    previousLabel: 'Previous 30 days',
    axis: timeAxis('day'),
    tooltip: formatFullDate,
    periodStart: dayStartOf,
  },
  year: {
    key: 'year',
    label: 'This year',
    title: 'This year',
    subtitle: '1-day buckets',
    radarSubtitle: 'the current year',
    pieSubtitle: 'over the current year',
    perChannelTitle: 'This year by channel',
    currentLabel: 'Current year',
    previousLabel: 'Last year',
    axis: timeAxis('month'),
    tooltip: formatFullDate,
    periodStart: yearStartOf,
  },
}

export const TIMEFRAME_ORDER: FixedTimeframe[] = ['day', 'week', 'last_30_days', 'year']

/// Shift every bucket to the same "position in period" axis so the current and
/// previous periods overlap: each series' own period start (from its first
/// bucket) is aligned onto the `anchor`. Returns the series unchanged when no
/// anchor is available (no data).
export function alignSeries(
  series: BarSeries[],
  anchor: number,
  periodStart: (time: number) => number,
): BarSeries[] {
  if (!Number.isFinite(anchor)) return series
  return series.map((item) => {
    const first = item.data[0]
    if (!first) return item
    const windowStart = periodStart(new Date(first.start).getTime())
    if (!Number.isFinite(windowStart)) return item
    return {
      ...item,
      data: item.data.map((bucket) => {
        const time = new Date(bucket.start).getTime()
        return { ...bucket, start: new Date(anchor + (time - windowStart)).toISOString() }
      }),
    }
  })
}

/// Aggregate (all-series) series for one timeframe: the current period plus the
/// previous period when the compare checkbox is on. Only needs the current and
/// previous buckets, so it works for both the detail and the summary graphs.
export function timeframeSeries(
  period: { current: TimeBucket[]; previous: TimeBucket[] },
  cfg: TimeframeConfig,
  compare: boolean,
): BarSeries[] {
  const series: BarSeries[] = []
  if (period.current.length > 0) {
    series.push({
      key: 'current',
      label: cfg.currentLabel,
      stackId: 'current',
      data: period.current,
    })
  }
  if (compare && period.previous.length > 0) {
    series.push({
      key: 'previous',
      label: cfg.previousLabel,
      stackId: 'previous',
      data: period.previous,
    })
  }
  return series
}

// ---------------------------------------------------------------------------
// "Individual" custom from/to range
// ---------------------------------------------------------------------------

/// The bucket resolutions the graphs can be aggregated into, keyed like the
/// backend's `resolution` query parameter: fixed-width 15/30-minute and hour
/// buckets plus calendar day/week/month/quarter buckets.
export type GranularityKey = '15m' | '30m' | 'hour' | 'day' | 'week' | 'month' | 'quarter'

/// Parses a `YYYY-MM-DD` input value into a browser-local `Date`.
export function dateFromInput(value: string): Date {
  const [year, month, day] = value.split('-').map(Number)
  return new Date(year, (month ?? 1) - 1, day ?? 1)
}

/// Formats a browser-local `Date` as a `YYYY-MM-DD` input value.
export function dateToInput(date: Date): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

/// Axis labels for week-aligned buckets: `dd.MM`.
const weekAxis = timeAxis('day')

/// Axis label for calendar-month buckets: `MMM yyyy`.
function monthAxis(time: number): string {
  return new Date(time).toLocaleDateString(LOCALE, { month: 'short', year: 'numeric' })
}

/// Axis label for calendar-quarter buckets: `Q<n> yyyy`.
function quarterAxis(time: number): string {
  const date = new Date(time)
  const quarter = Math.floor(date.getMonth() / 3) + 1
  return `Q${quarter} ${date.getFullYear()}`
}

/// Axis label for the overlapped week chart with day buckets: a short weekday
/// name marks each aligned day (one bucket per weekday, so no time suffix).
function weekdayDayAxis(time: number): string {
  return new Date(time).toLocaleDateString(LOCALE, { weekday: 'short' })
}

/// The chart-subtitle wording for a bucket resolution.
export function granularitySubtitle(granularity: GranularityKey): string {
  switch (granularity) {
    case '15m':
      return '15-minute buckets'
    case '30m':
      return '30-minute buckets'
    case 'hour':
      return '1-hour buckets'
    case 'day':
      return '1-day buckets'
    case 'week':
      return '1-week buckets'
    case 'month':
      return '1-month buckets'
    case 'quarter':
      return '1-quarter buckets'
  }
}

/// The tooltip formatter for a bucket resolution: sub-day buckets need the full
/// date-time, coarser buckets only the date.
function granularityTooltip(granularity: GranularityKey): (time: number) => string {
  return granularity === '15m' || granularity === '30m' || granularity === 'hour'
    ? formatFullDateTime
    : formatFullDate
}

/// The x-axis formatter for a resolution within a fixed timeframe. Week overlays
/// keep their weekday axis for sub-day buckets so weekday context is not lost;
/// the year view keeps its month labels at every resolution.
function fixedAxis(
  timeframe: FixedTimeframe,
  granularity: GranularityKey,
): (time: number) => string {
  const subDay = granularity === '15m' || granularity === '30m' || granularity === 'hour'
  switch (timeframe) {
    case 'week':
      // A single overlapped week keeps its weekday (+time) axis at every
      // resolution: sub-day buckets show `Mo HH:mm`, day buckets just `Mo`.
      return subDay ? weekdayAxis : weekdayDayAxis
    case 'year':
      // Month buckets land on one label per month; day/week buckets fall many
      // times inside one month, so consecutive duplicates are collapsed into a
      // single month label (the "JanJanFebFeb…" regression).
      return granularity === 'month' ? timeAxis('month') : dedupe(timeAxis('month'))
    case 'day':
      // A single 24 h window never repeats a time-of-day label.
      return timeAxis('hour')
    case 'last_30_days':
      // Hourly buckets across 30 days repeat `HH:mm` daily (after tick thinning
      // every visible tick lands on a day's `00:00`); prepend the day so every
      // label is unique. Day/week buckets stay on `dd.MM.`.
      return subDay ? dayTimeAxis : timeAxis('day')
  }
}

/// Whether a fixed timeframe's axis at a resolution emits long, dense labels
/// (e.g. `dd.MM., HH:mm` on every hour of the 30-day view) that need a -45°
/// rotation to stay readable. Short labels (`HH:mm`, `dd.MM.`, month names)
/// stay horizontal.
function fixedAxisRotate(timeframe: FixedTimeframe, granularity: GranularityKey): boolean {
  if (timeframe === 'week') {
    // 30-minute / 1-hour buckets over seven days carry long `Mo HH:mm` labels.
    return granularity === '30m' || granularity === 'hour'
  }
  return timeframe === 'last_30_days' && granularity === 'hour'
}

/// The presentation config of a fixed timeframe at a chosen resolution: the
/// timeframe's static labels/period math (from [`TIMEFRAMES`]) plus the
/// resolution-dependent subtitle, x-axis and tooltip.
export function fixedTimeframeConfig(
  timeframe: FixedTimeframe,
  granularity: GranularityKey,
): TimeframeConfig {
  const base = TIMEFRAMES[timeframe]
  return {
    ...base,
    subtitle: granularitySubtitle(granularity),
    axis: fixedAxis(timeframe, granularity),
    axisRotate: fixedAxisRotate(timeframe, granularity),
    tooltip: granularityTooltip(granularity),
  }
}

/// The presentation config of the "Individual" timeframe for a chosen
/// resolution. There is no previous period (compare is disabled), so
/// `previousLabel` is unused; the axis width covers the whole range.
export function customTimeframeConfig(
  granularity: GranularityKey,
  from: string,
  to: string,
): TimeframeConfig {
  const subDay = granularity === '15m' || granularity === '30m' || granularity === 'hour'
  const fromDate = dateFromInput(from)
  const toDate = dateFromInput(to)
  // Inclusive day count (00:00 `from` → 00:00 of the day after `to`), mirroring
  // the frontend span semantics that pick the resolution set.
  const days = (toDate.getTime() + 86_400_000 - fromDate.getTime()) / 86_400_000
  const crossesYear = fromDate.getFullYear() !== toDate.getFullYear()
  // Sub-day buckets repeat `HH:mm` every day once a range spans several days;
  // `dd.MM.` day/week buckets become dense and ambiguous once they cross a year.
  const axis = subDay
    ? days > 2
      ? dayTimeAxis
      : timeAxis('hour')
    : granularity === 'month'
      ? monthAxis
      : granularity === 'quarter'
        ? quarterAxis
        : crossesYear
          ? dayYearAxis
          : weekAxis
  // Sub-day labels with a day prefix and cross-year `dd.MM.yy` labels are long
  // enough to need a -45° rotation on a dense axis.
  const axisRotate = subDay ? days > 2 : granularity !== 'month' && granularity !== 'quarter'
  return {
    key: 'individual' as Timeframe,
    label: 'Individual',
    title: 'Individual range',
    subtitle: granularitySubtitle(granularity),
    radarSubtitle: 'over the selected range',
    pieSubtitle: 'over the selected range',
    perChannelTitle: 'Individual by channel',
    currentLabel: 'Selected range',
    previousLabel: '',
    axis,
    axisRotate,
    tooltip: granularityTooltip(granularity),
    // Identity period start: the aligned anchor is the first bucket (a custom
    // range has no previous-period overlay).
    periodStart: (time) => time,
  }
}
