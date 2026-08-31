import { withTrendParam } from './api'
import { useResource } from './useResource'
import type { PeriodGraphs } from './types'

/// Loads one timeframe of graph data from its HATEOAS link, appending the
/// Bike-Trends flag when enabled. The link changes when the timeframe changes,
/// so switching timeframes re-fetches only this card.
export function useStationGraphs(url: string | null, excludeNewStations: boolean) {
  return useResource<PeriodGraphs>(url ? withTrendParam(url, excludeNewStations) : null)
}
