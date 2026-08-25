import { useEffect, useState } from 'react'
import type { StationOverview } from './types'
import { fetchStationOverview } from './api'

/// Loads the overview payload for a station whenever its id changes.
export function useStationOverview(stationId: string | null) {
  const [overview, setOverview] = useState<StationOverview | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!stationId) {
      setOverview(null)
      setError(false)
      return
    }
    let cancelled = false
    setOverview(null)
    setError(false)
    fetchStationOverview(stationId)
      .then((data) => {
        if (!cancelled) setOverview(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
    return () => {
      cancelled = true
    }
  }, [stationId])

  return { overview, error }
}
