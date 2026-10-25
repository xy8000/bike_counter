import type { GlobalSummary, VersionResponse } from './types'

/// Load the whole-system summary for the header. With `excludeNewStations` (the
/// Bike-Trends setting) the "bikes / last day" total only counts stations that
/// already existed before the comparison day.
export async function fetchGlobalSummary(excludeNewStations: boolean): Promise<GlobalSummary> {
  const query = excludeNewStations ? '?exclude_new_stations=true' : ''
  const response = await fetch(`/api/bff/global-summary${query}`)
  if (!response.ok) throw new Error(`global summary responded with ${response.status}`)
  return response.json() as Promise<GlobalSummary>
}

/// Load the backend application version for the header version dialog.
export async function fetchBackendVersion(): Promise<string> {
  const response = await fetch('/api/bff/version')
  if (!response.ok) throw new Error(`version responded with ${response.status}`)
  const data = (await response.json()) as VersionResponse
  return data.version
}
