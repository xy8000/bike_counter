import { useResource } from './useResource'
import type { MonthTotal } from './types'

export interface MonthlyTotals {
  monthly_totals: MonthTotal[]
}

/// Loads the monthly totals card from its HATEOAS link.
export function useStationMonthly(url: string | null) {
  return useResource<MonthlyTotals>(url)
}
