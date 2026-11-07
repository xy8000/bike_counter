import { safePathSegment } from '../../lib/apiUrl'
import { getJson } from '../../lib/bff'
import type {
  DetailLinks,
  MonthTotal,
  PeriodGraphs,
  StationDetailPage,
  StationOverviewStats,
} from './types'

/// Fetch the detail page shell (metadata + channels) and derive its card links.
export async function fetchStationDetailPage(id: string): Promise<StationDetailPage> {
  const safeId = safePathSegment(id)
  const data = await getJson<Omit<StationDetailPage, '_links'>>(`/api/bff/station-detail/${safeId}`)
  return { ...data, _links: detailLinks(safeId) }
}

/// Every card link is built locally from the validated id. The shell's own
/// `_links.*.href` values are display-only and never become a request
/// destination, so a compromised response cannot steer a card request
/// (CSRF/SSRF). The cards fall back to the server's "now" reference instead of
/// the shell's `as_of` token.
function detailLinks(id: string): DetailLinks {
  const base = `/api/bff/station-detail/${id}`
  return {
    self: base,
    overview: `${base}/overview`,
    graphs_day: `${base}/graphs/day`,
    graphs_week: `${base}/graphs/week`,
    graphs_last_30_days: `${base}/graphs/last_30_days`,
    graphs_year: `${base}/graphs/year`,
    monthly: `${base}/monthly`,
  }
}

/// Appends the Bike-Trends flag to a detail card's HATEOAS link (which already
/// carries the `as_of` reference).
export function withTrendParam(url: string, excludeNewStations: boolean): string {
  if (!excludeNewStations) return url
  const separator = url.includes('?') ? '&' : '?'
  return `${url}${separator}exclude_new_stations=true`
}

/// Fetch one stats card by its HATEOAS link (the link carries the `as_of`
/// reference, so the URL is a pure function of the reference time).
export async function fetchOverviewStats(
  url: string,
  excludeNewStations: boolean,
): Promise<StationOverviewStats> {
  return getJson(withTrendParam(url, excludeNewStations))
}

/// Fetch one timeframe of graph data by its HATEOAS link.
export async function fetchGraphs(url: string, excludeNewStations: boolean): Promise<PeriodGraphs> {
  return getJson(withTrendParam(url, excludeNewStations))
}

/// Fetch the monthly totals card by its HATEOAS link.
export async function fetchMonthlyTotals(url: string): Promise<{ monthly_totals: MonthTotal[] }> {
  return getJson(url)
}
