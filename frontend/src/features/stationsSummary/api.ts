import type { Bounds } from '../../lib/geo'
import { bboxQuery } from '../../lib/geo'
import type {
  MonthlyTotals,
  StationsSummaryLinks,
  StationsSummaryOverview,
  StationsSummaryPage,
  SummaryPeriodGraphs,
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

/// Fetch the summary page shell for the stations inside `bounds` (bounds-only:
/// the shell does not depend on the disabled/exclude set).
export async function fetchStationsSummaryPage(bounds: Bounds): Promise<StationsSummaryPage> {
  const params = new URLSearchParams(bboxQuery(bounds))
  const data = await getJson<
    Omit<StationsSummaryPage, '_links'> & { _links: Record<string, RawLink> }
  >(`/api/bff/stations/summary?${params.toString()}`)
  return { ...data, _links: unwrapLinks<StationsSummaryLinks>(data._links) }
}

/// Appends the excluded station ids and the Bike-Trends flag to a summary card's
/// HATEOAS link (the link already carries the bounds + `as_of`).
function withSummaryParams(
  baseUrl: string,
  exclude: string[],
  excludeNewStations: boolean,
): string {
  let url = baseUrl
  if (exclude.length > 0) {
    const separator = url.includes('?') ? '&' : '?'
    url = `${url}${separator}exclude=${exclude.join(',')}`
  }
  if (excludeNewStations) {
    const separator = url.includes('?') ? '&' : '?'
    url = `${url}${separator}exclude_new_stations=true`
  }
  return url
}

/// Fetch one summary stats card by its HATEOAS link, including the local
/// exclude set and the Bike-Trends flag in the query.
export function fetchSummaryOverview(
  link: string,
  exclude: string[],
  excludeNewStations: boolean,
): Promise<StationsSummaryOverview> {
  return getJson(withSummaryParams(link, exclude, excludeNewStations))
}

export function fetchSummaryGraphs(
  link: string,
  exclude: string[],
  excludeNewStations: boolean,
): Promise<SummaryPeriodGraphs> {
  return getJson(withSummaryParams(link, exclude, excludeNewStations))
}

export function fetchSummaryMonthly(
  link: string,
  exclude: string[],
  excludeNewStations: boolean,
): Promise<MonthlyTotals> {
  return getJson(withSummaryParams(link, exclude, excludeNewStations))
}
