import { useEffect, useState } from 'react'
import type { Bounds } from '../../lib/geo'
import { fetchMapStations, fetchSidebarShell, fetchSidebarStats } from './api'
import type { SidebarShell, SidebarStationStats, StationMap } from './types'

/// Fetch the visible stations (map markers) + the sidebar shell whenever the
/// map bounds change, debounced so panning doesn't hammer the BFF. The shell
/// (identity + image) renders directly; the per-station stats are fetched in
/// parallel via the shell's HATEOAS `stats` link and fill in when they arrive.
///
/// A bounds change refetches for the new view, but previously loaded data stays
/// on screen while the request is in flight: the sidebar keeps the old list
/// instead of dropping back into skeleton "ghosts" (skeletons only render on the
/// very first load) and the map keeps its old markers until the new payload
/// replaces them in place.
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
      // The loading flag only drives the skeleton on the very first load (the
      // sidebar shows it when `shell === null`); on a refetch the previous
      // shell/stats/markers stay visible until the new data lands.
      setLoading(true)
      setError(false)
      setStatsError(false)
      // Map markers + shell in parallel; the shell is cheap, so the sidebar
      // identity renders as soon as it lands.
      Promise.all([fetchMapStations(bounds), fetchSidebarShell(bounds)])
        .then(([mapData, shellData]) => {
          if (cancelled) return
          // Replace the map markers only when the visible set actually changed,
          // so re-fetching the same stations (e.g. zooming around an unchanged
          // view) does not rebuild the cluster index and remount the markers.
          setMapStations((previous) =>
            previous !== null && sameMapStations(previous, mapData) ? previous : mapData,
          )
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

/// True when two map-marker arrays describe the same stations (a BFF refetch of
/// an unchanged view can come back with the same content in a different order).
/// Compares the full marker identity — id, name, status and position — so a
/// rename or status flip still triggers a replacement while a pure re-fetch of
/// the same set is skipped.
function sameMapStations(a: StationMap[], b: StationMap[]): boolean {
  if (a.length !== b.length) return false
  const signatures = new Set(a.map(stationSignature))
  return b.every((station) => signatures.has(stationSignature(station)))
}

function stationSignature(station: StationMap): string {
  return `${station.id}|${station.name}|${station.status}|${station.latitude}|${station.longitude}`
}
