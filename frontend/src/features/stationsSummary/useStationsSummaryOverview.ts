import { useEffect, useState } from 'react'
import { fetchSummaryOverview } from './api'
import type { StationsSummaryOverview } from './types'

/// Loads the overview card of the summary page. The URL depends on the shell's
/// HATEOAS link (bounds + `as_of`) plus the local exclude set, so toggling a
/// disabled station re-fetches only this card.
export function useStationsSummaryOverview(link: string | null, exclude: string[]) {
  const [overview, setOverview] = useState<StationsSummaryOverview | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)
  // Stable dependency key: a comma-joined list changes only when a station is
  // toggled, not on every render.
  const excludeKey = exclude.join(',')

  useEffect(() => {
    if (!link) return
    let cancelled = false
    setLoading(true)
    setOverview(null)
    setError(false)
    fetchSummaryOverview(link, exclude)
      .then((data) => {
        if (!cancelled) setOverview(data)
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

  return { overview, loading, error }
}
