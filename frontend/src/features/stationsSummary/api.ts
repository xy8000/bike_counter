import type { Bounds } from '../../lib/geo'
import { bboxQuery } from '../../lib/geo'
import { getJson, unwrapLinks, type RawLink } from '../../lib/bff'
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
