import { useEffect, useState } from 'react'
import type { Bounds } from '../../lib/geo'
import { fetchMapStations, fetchSidebarShell, fetchSidebarStats } from './api'
import type { SidebarShell, SidebarStationStats, StationMap } from './types'

/// Fetch the visible stations (map markers) + the sidebar shell whenever the
/// map bounds change, debounced so panning doesn't hammer the BFF. The shell
/// (identity + image) renders directly; the per-station stats are fetched in
/// parallel via the shell's HATEOAS `stats` link and fill in when they arrive.
export function useVisibleStations(bounds: Bounds | null) {
  const [mapStations, setMapStations] = useState<StationMap[] | null>(null)
  const [shell, setShell] = useState<SidebarShell | null>(null)
  const [stats, setStats] = useState<Map<string, SidebarStationStats> | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)
  const [statsError, setStatsError] = useState(false)

  useEffect(() => {
    if (!bounds) return
    let cancelled = false
    const timer = setTimeout(() => {
      setLoading(true)
      setShell(null)
      setStats(null)
      setError(false)
      setStatsError(false)
      // Map markers + shell in parallel; the shell is cheap, so the sidebar
      // identity renders as soon as it lands.
      Promise.all([fetchMapStations(bounds), fetchSidebarShell(bounds)])
        .then(([mapData, shellData]) => {
          if (cancelled) return
          setMapStations(mapData)
          setShell(shellData)
          setLoading(false)
          // The stats sub-resource runs in parallel with the shell's rendering
          // (it does not block the identity).
          fetchSidebarStats(shellData._links.stats)
            .then((statsData) => {
              if (cancelled) return
              setStats(new Map(statsData.items.map((item) => [item.station_id, item])))
            })
            .catch(() => {
              if (!cancelled) setStatsError(true)
            })
        })
        .catch(() => {
          if (!cancelled) {
            setError(true)
            setLoading(false)
          }
        })
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [bounds])

  return { mapStations, shell, stats, loading, error, statsError }
}
