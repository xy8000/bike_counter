import { useEffect, useState } from 'react'
import type { StationDetail } from './types'
import { fetchStationDetail } from './api'

/// Loads the detail payload for a station whenever its id changes.
export function useStationDetail(stationId: string | null) {
  const [detail, setDetail] = useState<StationDetail | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!stationId) {
      setDetail(null)
      setError(false)
      return
    }
    let cancelled = false
    setDetail(null)
    setError(false)
    fetchStationDetail(stationId)
      .then((data) => {
        if (!cancelled) setDetail(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
    return () => {
      cancelled = true
    }
  }, [stationId])

  return { detail, error }
}
