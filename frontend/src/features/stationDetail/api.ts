import type {
  DetailLinks,
  MonthTotal,
  PeriodGraphs,
  StationDetailPage,
  StationOverviewStats,
} from './types'

type RawLink = { href: string; templated?: boolean }

async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<T>
}

/// The BFF serializes each `_links` value as a `LinkDto` (`{ href, templated }`);
/// the frontend only needs the `href` string to fetch the card.
function unwrapLinks<T extends object>(links: Record<string, RawLink>): T {
  const out: Record<string, string> = {}
  for (const [key, value] of Object.entries(links)) {
    out[key] = value.href
  }
  return out as unknown as T
}

/// Fetch the detail page shell (metadata + channels + HATEOAS links).
export async function fetchStationDetailPage(id: string): Promise<StationDetailPage> {
  const data = await getJson<
    Omit<StationDetailPage, '_links'> & { _links: Record<string, RawLink> }
  >(`/api/bff/station-detail/${id}`)
  return { ...data, _links: unwrapLinks<DetailLinks>(data._links) }
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
export function fetchOverviewStats(
  url: string,
  excludeNewStations: boolean,
): Promise<StationOverviewStats> {
  return getJson(withTrendParam(url, excludeNewStations))
}

/// Fetch one timeframe of graph data by its HATEOAS link.
export function fetchGraphs(url: string, excludeNewStations: boolean): Promise<PeriodGraphs> {
  return getJson(withTrendParam(url, excludeNewStations))
}

/// Fetch the monthly totals card by its HATEOAS link.
export function fetchMonthlyTotals(url: string): Promise<{ monthly_totals: MonthTotal[] }> {
  return getJson(url)
}
