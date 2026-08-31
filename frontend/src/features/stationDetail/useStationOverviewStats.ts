import { withTrendParam } from './api'
import { useResource } from './useResource'
import type { StationOverviewStats } from './types'

/// Loads the overview stats card from its HATEOAS link (which carries the
/// `as_of` reference), appending the Bike-Trends flag when enabled.
export function useStationOverviewStats(url: string | null, excludeNewStations: boolean) {
  return useResource<StationOverviewStats>(url ? withTrendParam(url, excludeNewStations) : null)
}
