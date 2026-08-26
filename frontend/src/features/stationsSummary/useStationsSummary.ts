import { useEffect, useState } from 'react'
import type { Bounds } from '../../lib/geo'
import { fetchStationsSummary } from './api'
import type { StationsSummary } from './types'

/// Loads the aggregated station-summary page whenever the bounds or the set of
/// disabled stations change. The page can take a while, so a `loading` flag is
/// exposed for the loading state.
export function useStationsSummary(bounds: Bounds | null, disabled: string[]) {
  const [summary, setSummary] = useState<StationsSummary | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)
  // Stable dependency key: a comma-joined list changes only when a station is
  // toggled, not on every render.
  const disabledKey = disabled.join(',')

  useEffect(() => {
    if (!bounds) return
    let cancelled = false
    setLoading(true)
    setSummary(null)
    setError(false)
    fetchStationsSummary(bounds, disabled)
      .then((data) => {
        if (!cancelled) setSummary(data)
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
  }, [bounds, disabledKey])

  return { summary, loading, error }
}
