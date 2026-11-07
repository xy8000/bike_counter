import { useEffect, useState } from 'react'
import type { StationOverviewPage, StationOverviewStats } from './types'
import { fetchStationOverview, fetchStationOverviewStats } from './api'

/// Loads the overview shell (identity — the panel renders the name immediately)
/// and then the stats card, so the two load and fail independently. Both request
/// paths are built locally from the station id; the shell's HATEOAS `_links` are
/// display-only and never become request destinations.
export function useStationOverview(stationId: string | null) {
  const [page, setPage] = useState<StationOverviewPage | null>(null)
  const [stats, setStats] = useState<StationOverviewStats | null>(null)
  const [error, setError] = useState(false)
  const [statsError, setStatsError] = useState(false)

  useEffect(() => {
    if (!stationId) {
      setPage(null)
      setStats(null)
      setError(false)
      setStatsError(false)
      return
    }
    let cancelled = false
    setPage(null)
    setStats(null)
    setError(false)
    setStatsError(false)
    // The stats sub-resource loads once the shell has provided the identity, but
    // its failure is tracked separately so the two cards fail independently.
    const load = async () => {
      try {
        const data = await fetchStationOverview(stationId)
        if (cancelled) return
        setPage(data)
        try {
          const statsData = await fetchStationOverviewStats(data.id)
          if (!cancelled) setStats(statsData)
        } catch {
          if (!cancelled) setStatsError(true)
        }
      } catch {
        if (!cancelled) setError(true)
      }
    }
    void load()
    return () => {
      cancelled = true
    }
  }, [stationId])

  return { page, stats, error, statsError }
}
