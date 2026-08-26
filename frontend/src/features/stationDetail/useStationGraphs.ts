import { useResource } from './useResource'
import type { PeriodGraphs } from './types'

/// Loads one timeframe of graph data from its HATEOAS link. The link changes
/// when the timeframe changes, so switching timeframes re-fetches only this
/// card.
export function useStationGraphs(url: string | null) {
  return useResource<PeriodGraphs>(url)
}
