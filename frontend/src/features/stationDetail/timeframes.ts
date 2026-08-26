/// Shared timeframe presentation for the detail page and the station-summary
/// page: the selectable timeframes, the axis/tooltip formatters and the
/// period-alignment helpers. Both pages render the same charts, so the config
/// lives here instead of being duplicated.

import { formatFullDate, formatFullDateTime, LOCALE } from '../../lib/format'
import type { TimeBucket, Timeframe } from './types'
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
/// Monday, so a short weekday name marks each day.
function weekdayAxis(time: number): string {
  return new Date(time).toLocaleDateString(LOCALE, { weekday: 'short' })
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

export const TIMEFRAMES: Record<Timeframe, TimeframeConfig> = {
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

export const TIMEFRAME_ORDER: Timeframe[] = ['day', 'week', 'last_30_days', 'year']

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
