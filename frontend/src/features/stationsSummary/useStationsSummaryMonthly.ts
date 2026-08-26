import { useEffect, useState } from 'react'
import { fetchSummaryMonthly } from './api'
import type { MonthlyTotals } from './types'

/// Loads the monthly totals card of the summary page. The URL depends on the
/// shell's HATEOAS link plus the local exclude set, so toggling a disabled
/// station re-fetches only this card.
export function useStationsSummaryMonthly(link: string | null, exclude: string[]) {
  const [monthly, setMonthly] = useState<MonthlyTotals | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)
  const excludeKey = exclude.join(',')

  useEffect(() => {
    if (!link) return
    let cancelled = false
    setLoading(true)
    setMonthly(null)
    setError(false)
    fetchSummaryMonthly(link, exclude)
      .then((data) => {
        if (!cancelled) setMonthly(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [link, excludeKey])

  return { monthly, loading, error }
}
