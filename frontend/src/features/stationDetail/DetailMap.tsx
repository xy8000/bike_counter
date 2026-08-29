import { Marker } from '@vis.gl/react-maplibre'
import { useNavigate } from 'react-router-dom'
import type { Bounds } from '../../lib/geo'
import { mapBounds, serializeBounds } from '../../lib/geo'
import { stationMarkerImage } from '../../lib/map'
import { BaseMap } from '../map/BaseMap'

/// A small, non-interactive MapLibre preview centred on the station with a
/// highlighted marker. Clicking it opens the map view at the preview's visible
/// bounds (a history push), so the browser "back" event re-routes to this
/// station.
export function DetailMap({
  latitude,
  longitude,
  name,
}: {
  latitude: number | null
  longitude: number | null
  name: string
}) {
  const navigate = useNavigate()

  if (latitude === null || longitude === null) {
    return (
      <div className="flex h-64 w-full items-center justify-center rounded-lg border bg-muted p-4 text-center text-sm text-muted-foreground md:h-80">
        No map position available for this station.
      </div>
    )
  }

  const openMap = (bounds: Bounds) => {
    const params = serializeBounds(bounds)
    navigate(`/?${params.toString()}`)
  }

  return (
    <div
      className="h-64 w-full overflow-hidden rounded-lg border md:h-80"
      aria-label="Map preview"
      title="Open the map at this view"
    >
      <BaseMap
        interactive={false}
        initialViewState={{ longitude, latitude, zoom: 15 }}
        onVoidClick={(map) => openMap(mapBounds(map))}
      >
        <Marker longitude={longitude} latitude={latitude}>
          {stationMarkerImage(name, { large: true })}
        </Marker>
      </BaseMap>
    </div>
  )
}
