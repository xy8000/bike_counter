import type { GlobalSummary } from './types'

/// Load the whole-system summary for the header. With `excludeNewStations` (the
/// Bike-Trends setting) the "bikes / last day" total only counts stations with
/// data covering the whole last day and its comparison day.
export async function fetchGlobalSummary(excludeNewStations: boolean): Promise<GlobalSummary> {
  const query = excludeNewStations ? '?exclude_new_stations=true' : ''
  const response = await fetch(`/api/bff/global-summary${query}`)
  if (!response.ok) throw new Error(`global summary responded with ${response.status}`)
  return response.json() as Promise<GlobalSummary>
}
