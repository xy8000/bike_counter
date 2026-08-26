/// Types for the counting-station overview page (BFF `station-overview/{id}`).

export type Trend = 'up' | 'down' | 'flat'

export interface StationOverviewMetric {
  key: 'last_day' | 'last_7_days' | 'last_month' | 'last_year'
  current: number
  previous: number
  trend: Trend
  delta_percent: number | null
}

/// The page-shaped payload everything the overview panel needs to render.
export interface StationOverview {
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
  detail_url: string
}
