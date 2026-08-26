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

/// The HATEOAS links of the detail page shell: one URL per stats card. The
/// windowed links (`overview`, `graphs_*`) carry the `as_of` reference.
export interface DetailLinks {
  self: string
  overview: string
  graphs_day: string
  graphs_week: string
  graphs_last_30_days: string
  graphs_year: string
  monthly: string
}

/// The page-shell payload of the detail page: the station metadata + channels
/// the layout needs, plus the `_links` to each stats card. Each card is fetched
/// on its own.
export interface StationDetailPage {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  channel_count: number
  image_url: string
  last_update: string | null
  channels: ChannelRef[]
  _links: DetailLinks
}

/// Maps a `Timeframe` to its HATEOAS link key on the detail shell.
export const GRAPH_LINK_KEYS: Record<Timeframe, keyof DetailLinks> = {
  day: 'graphs_day',
  week: 'graphs_week',
  last_30_days: 'graphs_last_30_days',
  year: 'graphs_year',
}

/// The overview card: the all-time total and the four trend metrics.
export interface StationOverviewStats {
  total_bikes: number
  metrics: StationOverviewMetric[]
}
