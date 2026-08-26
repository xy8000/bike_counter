import { useEffect, useState } from 'react'
import type { Bounds } from '../../lib/geo'
import { fetchStationsSummaryPage } from './api'
import type { StationsSummaryPage } from './types'

/// Loads the summary page shell whenever the bounds change. The shell is
/// bounds-only: it does not depend on the disabled/exclude set, so toggling a
/// station on the map never re-fetches it.
export function useStationsSummaryPage(bounds: Bounds | null) {
  const [page, setPage] = useState<StationsSummaryPage | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!bounds) return
    let cancelled = false
    setLoading(true)
    setPage(null)
    setError(false)
    fetchStationsSummaryPage(bounds)
      .then((data) => {
        if (!cancelled) setPage(data)
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
  }, [bounds])

  return { page, loading, error }
}
