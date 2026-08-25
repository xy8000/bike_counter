import type { StationDetail } from './types'

/// Fetch the page-shaped detail payload for one counting station.
export async function fetchStationDetail(id: string): Promise<StationDetail> {
  const url = `/api/bff/station-detail/${id}`
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<StationDetail>
}
