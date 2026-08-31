/// Types for the station-summary page (BFF `stations/summary`).
import type { StationOverviewMetric } from '../stationOverview/types'
import type {
  FixedTimeframe,
  HourTotal,
  MonthTotal,
  TimeBucket,
  WeekdayTotal,
} from '../stationDetail/types'

/// A positioned station inside the requested bounds (disabled ones included so
/// the map can gray them out).
export interface SummaryStation {
  id: string
  name: string
  latitude: number
  longitude: number
  channel_count: number
}

/// One station's share over the current period (summary pie chart).
export interface StationTotal {
  station_id: string
  total: number
}

/// The graphs restricted to a single station (summary nerd stats).
export interface PerStationSeries {
  station_id: string
  current: TimeBucket[]
  previous: TimeBucket[]
  weekday_radar: WeekdayTotal[]
  weekday_radar_previous: WeekdayTotal[]
  hourly: HourTotal[]
  hourly_previous: HourTotal[]
}

/// The graph data for one timeframe of the summary page: the aggregate current
/// and previous period time-series, the current-period weekday radar + station
/// pie and the per-station series.
export interface SummaryPeriodGraphs {
  current: TimeBucket[]
  previous: TimeBucket[]
  weekday_radar: WeekdayTotal[]
  weekday_radar_previous: WeekdayTotal[]
  hourly: HourTotal[]
  hourly_previous: HourTotal[]
  station_pie: StationTotal[]
  per_station: PerStationSeries[]
}

/// The HATEOAS links of the summary page shell: one URL per stats card. The
/// windowed links (`overview`, `graphs_*`) carry the bounds + `as_of`; the
/// frontend appends `&exclude=...` from its local disabled state.
export interface StationsSummaryLinks {
  self: string
  overview: string
  graphs_day: string
  graphs_week: string
  graphs_last_30_days: string
  graphs_year: string
  monthly: string
}

/// Maps a fixed `Timeframe` to its HATEOAS link key on the summary shell. The
/// `individual` timeframe uses the `graphs_day` link as its base and appends
/// the custom `from`/`to` range.
export const GRAPH_LINK_KEYS: Record<FixedTimeframe, keyof StationsSummaryLinks> = {
  day: 'graphs_day',
  week: 'graphs_week',
  last_30_days: 'graphs_last_30_days',
  year: 'graphs_year',
}

/// The page-shell payload of the summary page: the fallback image, the station
/// list (for the map + toggle) and the last update, plus the `_links` to each
/// stats card. The aggregated stats live in the cards because they depend on the
/// `exclude` set.
export interface StationsSummaryPage {
  image_url: string
  stations: SummaryStation[]
  last_update: string | null
  _links: StationsSummaryLinks
}

/// The overview card of the summary page: the aggregated channel count, all-time
/// total and four trend metrics over the included stations.
export interface StationsSummaryOverview {
  channel_count: number
  total_bikes: number
  metrics: StationOverviewMetric[]
}

/// The monthly totals card of the summary page.
export interface MonthlyTotals {
  monthly_totals: MonthTotal[]
}
