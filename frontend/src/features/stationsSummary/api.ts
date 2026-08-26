import type { Bounds } from '../../lib/geo'
import { bboxQuery } from '../../lib/geo'
import type { StationsSummary } from './types'

/// Fetch the aggregated station-summary page for the stations inside `bounds`,
/// excluding the given station ids from the aggregation.
export async function fetchStationsSummary(
  bounds: Bounds,
  exclude: string[],
): Promise<StationsSummary> {
  const params = new URLSearchParams(bboxQuery(bounds))
  if (exclude.length > 0) {
    params.set('exclude', exclude.join(','))
  }
  const url = `/api/bff/stations/summary?${params.toString()}`
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<StationsSummary>
}
