import type { StationOverview } from './types'

/// Fetch the page-shaped overview payload for one counting station.
export async function fetchStationOverview(id: string): Promise<StationOverview> {
  const url = `/api/bff/station-overview/${id}`
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<StationOverview>
}
