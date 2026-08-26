import { useResource } from './useResource'
import type { StationOverviewStats } from './types'

/// Loads the overview stats card from its HATEOAS link (which carries the
/// `as_of` reference).
export function useStationOverviewStats(url: string | null) {
  return useResource<StationOverviewStats>(url)
}
