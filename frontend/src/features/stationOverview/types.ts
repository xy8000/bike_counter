/// Types for the counting-station overview page (BFF `station-overview/{id}`).

export type Trend = 'up' | 'down' | 'flat'

export interface StationOverviewMetric {
  key: 'last_day' | 'last_7_days' | 'last_month' | 'last_year'
  current: number
  previous: number
  trend: Trend
  delta_percent: number | null
}

/// The overview **shell** (from GET /api/bff/station-overview/{id}): the
/// station identity the panel needs to render the name/image immediately, plus
/// a HATEOAS `stats` link to the stats sub-resource.
export interface StationOverviewPage {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  channel_count: number
  image_url: string
  last_update: string | null
  detail_url: string
  _links: {
    stats: string
  }
}

/// The overview stats card (from GET /api/bff/station-overview/{id}/stats): the
/// all-time total and the four trend metrics.
export interface StationOverviewStats {
  total_bikes: number
  metrics: StationOverviewMetric[]
}
