/// Types for the counting-station detail page (BFF `station-detail/{id}`).
import type { StationOverviewMetric } from '../stationOverview/types'

export interface ChannelRef {
  id: string
  name: string
}

/// One fixed-width time-bucket of an aggregate sum (ISO-8601 UTC start).
export interface TimeBucket {
  start: string
  total: number
}

/// One weekday aggregate (ISO 1 = Monday .. 7 = Sunday).
export interface WeekdayTotal {
  weekday: number
  total: number
}

/// One hour-of-day aggregate (local 0 = midnight .. 23 = 23:00).
export interface HourTotal {
  hour: number
  total: number
}

/// One channel's share over the current period (pie chart).
export interface ChannelTotal {
  channel_id: string
  total: number
}

/// Total per local calendar month (year + ISO month 1..=12) over the whole
/// history (monthly bar chart).
export interface MonthTotal {
  year: number
  month: number
  total: number
}

/// The selectable timeframes driven by the shared dropdown.
export type Timeframe = 'day' | 'week' | 'last_30_days' | 'year'

/// The per-channel time-series for one timeframe (nerd stats).
export interface PerChannelSeries {
  channel_id: string
  current: TimeBucket[]
  previous: TimeBucket[]
  weekday_radar: WeekdayTotal[]
  weekday_radar_previous: WeekdayTotal[]
  hourly: HourTotal[]
  hourly_previous: HourTotal[]
}

/// All graph data for one timeframe: the current and previous period
/// time-series, the current period's weekday radar + channel pie, and the
/// per-channel series.
export interface PeriodGraphs {
  current: TimeBucket[]
  previous: TimeBucket[]
  weekday_radar: WeekdayTotal[]
  weekday_radar_previous: WeekdayTotal[]
  hourly: HourTotal[]
  hourly_previous: HourTotal[]
  channel_pie: ChannelTotal[]
  per_channel: PerChannelSeries[]
}

/// All graph data for the detail page, keyed by the four timeframes, plus the
/// per-month totals for the standalone monthly bar chart.
export interface StationDetailGraphs {
  day: PeriodGraphs
  week: PeriodGraphs
  last_30_days: PeriodGraphs
  year: PeriodGraphs
  monthly_totals: MonthTotal[]
}

/// The page-shaped payload for the detail page: station metadata (same fields as
/// the overview, plus the YEAR stat in `metrics`), channel references and all
/// graph data.
export interface StationDetail {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  channel_count: number
  /// All-time total of bikes counted at this station (the whole history).
  total_bikes: number
  image_url: string
  metrics: StationOverviewMetric[]
  last_update: string | null
  channels: ChannelRef[]
  graphs: StationDetailGraphs
}
