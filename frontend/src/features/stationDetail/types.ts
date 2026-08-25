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

/// One channel's share over the last 30 days (pie chart).
export interface ChannelTotal {
  channel_id: string
  total: number
}

/// The per-channel time-series (nerd stats): the time windows restricted to one
/// channel.
export interface PerChannelSeries {
  channel_id: string
  weekday_radar: WeekdayTotal[]
  last_day: TimeBucket[]
  current_week: TimeBucket[]
  last_week: TimeBucket[]
  last_30_days: TimeBucket[]
  current_year: TimeBucket[]
  last_year: TimeBucket[]
}

/// All graph data for the detail page.
export interface StationDetailGraphs {
  last_day: TimeBucket[]
  weekday_radar: WeekdayTotal[]
  current_week: TimeBucket[]
  last_week: TimeBucket[]
  last_30_days: TimeBucket[]
  current_year: TimeBucket[]
  last_year: TimeBucket[]
  per_channel: PerChannelSeries[]
  channel_pie: ChannelTotal[]
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
  image_url: string
  metrics: StationOverviewMetric[]
  last_update: string | null
  channels: ChannelRef[]
  graphs: StationDetailGraphs
}
