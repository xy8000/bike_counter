import { useEffect, useState } from 'react'
import type { Bounds } from '../../lib/geo'
import { fetchVisibleStations } from './api'
import type { StationMap, StationSummarySidebar } from './types'

/// Fetch the visible stations (map markers) + sidebar summaries whenever the
/// map bounds change, debounced so panning doesn't hammer the BFF.
export function useVisibleStations(bounds: Bounds | null) {
  const [mapStations, setMapStations] = useState<StationMap[] | null>(null)
  const [sidebar, setSidebar] = useState<StationSummarySidebar | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!bounds) return
    const timer = setTimeout(() => {
      fetchVisibleStations(bounds)
        .then(({ mapStations, sidebar }) => {
          setMapStations(mapStations)
          setSidebar(sidebar)
          setError(false)
        })
        .catch(() => setError(true))
    }, 250)
    return () => clearTimeout(timer)
  }, [bounds])

  return { mapStations, sidebar, error }
}
