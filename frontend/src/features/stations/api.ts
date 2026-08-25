import type { Bounds } from '../../lib/geo'
import { bboxQuery } from '../../lib/geo'
import type { StationMapList, StationSearch, StationSummarySidebar } from './types'

async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<T>
}

/// Fetch the visible map markers + the sidebar summaries for the given bounds.
export async function fetchVisibleStations(bounds: Bounds): Promise<{
  mapStations: StationMapList['items']
  sidebar: StationSummarySidebar
}> {
  const query = bboxQuery(bounds)
  const [mapData, sidebarData] = await Promise.all([
    getJson<StationMapList>(`/api/bff/stations?${query}`),
    getJson<StationSummarySidebar>(`/api/bff/stations/sidebar?${query}`),
  ])
  return { mapStations: mapData.items, sidebar: sidebarData }
}

/// Fetch every station plus the action map for the search dialog.
export async function fetchStationSearch(): Promise<StationSearch> {
  return getJson<StationSearch>('/api/bff/stations/search')
}
