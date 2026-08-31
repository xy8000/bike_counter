/// Shared timeframe presentation for the detail page and the station-summary
/// page: the selectable timeframes, the axis/tooltip formatters and the
/// period-alignment helpers. Both pages render the same charts, so the config
/// lives here instead of being duplicated.

import { formatFullDate, formatFullDateTime, LOCALE } from '../../lib/format'
import type { FixedTimeframe, TimeBucket, Timeframe } from './types'
import type { LineSeries } from './TimeSeriesLineChart'

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
  tooltip: (time: number) => string
  periodStart: (time: number) => number
  domainWidthMs?: number
}

const HOUR_MS = 3_600_000
const DAY_MS = 24 * HOUR_MS

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
    domainWidthMs: DAY_MS,
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
    domainWidthMs: 7 * DAY_MS,
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
    domainWidthMs: 30 * DAY_MS,
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

/// The x-axis domain of the selected timeframe, anchored on the current period's
/// start. The year timeframe runs to the actual next local Jan 1; the others use
/// a fixed width (DST days are a couple of minutes short/long, which is fine for
/// display).
export function timeframeDomain(
  cfg: TimeframeConfig,
  anchor: number,
): [number, number] | undefined {
  if (!Number.isFinite(anchor)) return undefined
  if (cfg.key === 'year') {
    const yearEnd = new Date(new Date(anchor).getFullYear() + 1, 0, 1).getTime()
    return [anchor, yearEnd]
  }
  return [anchor, anchor + (cfg.domainWidthMs ?? 0)]
}

/// Shift every bucket to the same "position in period" axis so the current and
/// previous periods overlap: each series' own period start (from its first
/// bucket) is aligned onto the `anchor`. Returns the series unchanged when no
/// anchor is available (no data).
export function alignSeries(
  series: LineSeries[],
  anchor: number,
  periodStart: (time: number) => number,
): LineSeries[] {
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
): LineSeries[] {
  const series: LineSeries[] = []
  if (period.current.length > 0) {
    series.push({ key: 'current', label: cfg.currentLabel, data: period.current })
  }
  if (compare && period.previous.length > 0) {
    series.push({ key: 'previous', label: cfg.previousLabel, data: period.previous })
  }
  return series
}

// ---------------------------------------------------------------------------
// "Individual" custom from/to range
// ---------------------------------------------------------------------------

/// The bucket resolution of a custom from/to range, mirroring the backend's
/// `custom_granularity`: `<= 24h` → 15 minutes, `<= 48h` → 1 hour, `<= 30d` →
/// 1 day, `<= 90d` → 1 week, `<= 2y` → 1 month, otherwise → 1 quarter.
export type CustomResolution = '15m' | 'hour' | 'day' | 'week' | 'month' | 'quarter'

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

/// The bucket resolution for an inclusive custom range `[from, to]` (the API
/// range runs to the day after `to`, so the span equals the selected day count).
export function customResolution(from: Date, to: Date): CustomResolution {
  const toExclusive = new Date(to.getFullYear(), to.getMonth(), to.getDate() + 1)
  const spanMs = toExclusive.getTime() - from.getTime()
  const hours = spanMs / 3_600_000
  const days = spanMs / 86_400_000
  if (hours <= 24) return '15m'
  if (hours <= 48) return 'hour'
  if (days <= 30) return 'day'
  if (days <= 90) return 'week'
  if (days <= 730) return 'month'
  return 'quarter'
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

/// The presentation config of the "Individual" timeframe, derived from the
/// selected from/to range. There is no previous period (compare is disabled),
/// so `previousLabel` is unused; the axis width covers the whole range.
export function customTimeframeConfig(from: Date, to: Date): TimeframeConfig {
  const resolution = customResolution(from, to)
  const subtitle =
    resolution === '15m'
      ? '15-minute buckets'
      : resolution === 'hour'
        ? '1-hour buckets'
        : resolution === 'week'
          ? '1-week buckets'
          : resolution === 'month'
            ? '1-month buckets'
            : resolution === 'quarter'
              ? '1-quarter buckets'
              : '1-day buckets'
  const axis =
    resolution === '15m' || resolution === 'hour'
      ? timeAxis('hour')
      : resolution === 'month'
        ? monthAxis
        : resolution === 'quarter'
          ? quarterAxis
          : weekAxis
  const tooltip =
    resolution === '15m' || resolution === 'hour' ? formatFullDateTime : formatFullDate
  const toExclusive = new Date(to.getFullYear(), to.getMonth(), to.getDate() + 1)
  return {
    key: 'individual' as Timeframe,
    label: 'Individual',
    title: 'Individual range',
    subtitle,
    radarSubtitle: 'over the selected range',
    pieSubtitle: 'over the selected range',
    perChannelTitle: 'Individual by channel',
    currentLabel: 'Selected range',
    previousLabel: '',
    axis,
    tooltip,
    // Identity period start: the aligned anchor is the first bucket, and the
    // domain spans the whole selected range (no previous overlay).
    periodStart: (time) => time,
    domainWidthMs: toExclusive.getTime() - from.getTime(),
  }
}
