import { safePathSegment } from '../../lib/apiUrl'
import { getJson } from '../../lib/bff'
import type { Bounds } from '../../lib/geo'
import { bboxQuery } from '../../lib/geo'
import type {
  MonthlyTotals,
  StationsSummaryLinks,
  StationsSummaryOverview,
  StationsSummaryPage,
  SummaryPeriodGraphs,
} from './types'

/// Fetch the summary page shell for the stations inside `bounds` (bounds-only:
/// the shell does not depend on the disabled/exclude set).
export async function fetchStationsSummaryPage(bounds: Bounds): Promise<StationsSummaryPage> {
  const query = bboxQuery(bounds)
  const data = await getJson<Omit<StationsSummaryPage, '_links'>>(
    `/api/bff/stations/summary?${query}`,
  )
  return { ...data, _links: summaryLinks(query) }
}

/// Every card link is built locally from the same bounds query; the shell's
/// `_links.*.href` values are display-only, so a compromised response cannot
/// steer a card request (CSRF/SSRF). The cards fall back to the server's "now"
/// reference instead of the shell's `as_of` token.
function summaryLinks(query: string): StationsSummaryLinks {
  const base = '/api/bff/stations/summary'
  return {
    self: `${base}?${query}`,
    overview: `${base}/overview?${query}`,
    graphs_day: `${base}/graphs/day?${query}`,
    graphs_week: `${base}/graphs/week?${query}`,
    graphs_last_30_days: `${base}/graphs/last_30_days?${query}`,
    graphs_year: `${base}/graphs/year?${query}`,
    monthly: `${base}/monthly?${query}`,
  }
}

/// Appends the excluded station ids and the Bike-Trends flag to a summary card's
/// HATEOAS link (the link already carries the bounds + `as_of`). Every id is
/// validated as one plain path segment first, so an id can never smuggle an
/// extra query key (`&…`) or delimiter into the request.
function withSummaryParams(
  baseUrl: string,
  exclude: string[],
  excludeNewStations: boolean,
): string {
  let url = baseUrl
  if (exclude.length > 0) {
    const separator = url.includes('?') ? '&' : '?'
    url = `${url}${separator}exclude=${exclude.map(safePathSegment).join(',')}`
  }
  if (excludeNewStations) {
    const separator = url.includes('?') ? '&' : '?'
    url = `${url}${separator}exclude_new_stations=true`
  }
  return url
}

/// Fetch one summary stats card by its HATEOAS link, including the local
/// exclude set and the Bike-Trends flag in the query.
export async function fetchSummaryOverview(
  link: string,
  exclude: string[],
  excludeNewStations: boolean,
): Promise<StationsSummaryOverview> {
  return getJson(withSummaryParams(link, exclude, excludeNewStations))
}

export async function fetchSummaryGraphs(
  link: string,
  exclude: string[],
  excludeNewStations: boolean,
): Promise<SummaryPeriodGraphs> {
  return getJson(withSummaryParams(link, exclude, excludeNewStations))
}

export async function fetchSummaryMonthly(
  link: string,
  exclude: string[],
  excludeNewStations: boolean,
): Promise<MonthlyTotals> {
  return getJson(withSummaryParams(link, exclude, excludeNewStations))
}
