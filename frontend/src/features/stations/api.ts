import type { Bounds } from '../../lib/geo'
import { bboxQuery } from '../../lib/geo'
import type {
  SidebarShell,
  SidebarStats,
  StationMapList,
  StationSearch,
} from './types'

type RawLink = { href: string; templated?: boolean }

async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<T>
}

/// Fetch the map markers for the given bounds.
export async function fetchMapStations(bounds: Bounds): Promise<StationMapList['items']> {
  const query = bboxQuery(bounds)
  const data = await getJson<StationMapList>(`/api/bff/stations?${query}`)
  return data.items
}

/// Fetch the sidebar shell (identity + image_url + counters + stats link). The
/// BFF serializes `_links.stats` as a `LinkDto` (`{ href, templated }`); the
/// frontend only needs the `href` string to fetch the stats sub-resource.
export async function fetchSidebarShell(bounds: Bounds): Promise<SidebarShell> {
  const query = bboxQuery(bounds)
  const data = await getJson<
    Omit<SidebarShell, '_links'> & { _links: Record<string, RawLink> }
  >(`/api/bff/stations/sidebar?${query}`)
  return { ...data, _links: { stats: data._links.stats.href } }
}

/// Fetch the per-station stats via the shell's stats link (which carries the
/// current bounds).
export async function fetchSidebarStats(statsUrl: string): Promise<SidebarStats> {
  return getJson<SidebarStats>(statsUrl)
}

/// Fetch every station plus the action map for the search dialog.
export async function fetchStationSearch(): Promise<StationSearch> {
  return getJson<StationSearch>('/api/bff/stations/search')
}
