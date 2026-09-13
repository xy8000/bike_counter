import { getJson, type RawLink } from '../../lib/bff'
import type { StationOverviewPage, StationOverviewStats } from './types'

/// Fetch the overview shell (identity + HATEOAS stats link) for one counting
/// station. The BFF serializes `_links.stats` as a `LinkDto` (`{ href,
/// templated }`); the frontend only needs the `href` string.
export async function fetchStationOverview(id: string): Promise<StationOverviewPage> {
  const url = `/api/bff/station-overview/${id}`
  const data = await getJson<
    Omit<StationOverviewPage, '_links'> & { _links: Record<string, RawLink> }
  >(url)
  return { ...data, _links: { stats: data._links.stats.href } }
}

/// Fetch the overview stats card (all-time total + four metrics) via the
/// shell's stats link.
export async function fetchStationOverviewStats(url: string): Promise<StationOverviewStats> {
  return getJson<StationOverviewStats>(url)
}
