import { useEffect, useState } from 'react'
import { fetchStationDetailPage } from './api'
import type { StationDetailPage } from './types'

/// Loads the detail page shell (metadata + channels + HATEOAS links) whenever
/// the station id changes. The API layer unwraps the `_links` hrefs so the
/// cards can fetch their sub-resources.
export function useStationDetailPage(stationId: string | null) {
  const [page, setPage] = useState<StationDetailPage | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!stationId) {
      setPage(null)
      setError(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setPage(null)
    setError(false)
    fetchStationDetailPage(stationId)
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
  }, [stationId])

  return { data: page, loading, error }
}
