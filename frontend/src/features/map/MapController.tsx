import { useEffect } from 'react'
import { useMap, useMapEvents } from 'react-leaflet'
import type { Map as LeafletMap } from 'leaflet'
import type { Bounds } from '../../lib/geo'
import { mapBounds } from '../../lib/geo'

/// Bridges Leaflet's map events to React state: reports the current bounding
/// box on mount and on every `moveend`, and hands the map instance to the app.
export function MapController({
  onBounds,
  onReady,
}: {
  onBounds: (bounds: Bounds) => void
  onReady: (map: LeafletMap) => void
}) {
  const map = useMap()

  useEffect(() => {
    onReady(map)
    onBounds(mapBounds(map))
    // Only run once, when the map instance is first available.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map])

  useMapEvents({
    moveend: () => onBounds(mapBounds(map)),
  })

  return null
}
