import { MapContainer, Marker, TileLayer, useMap, useMapEvents } from 'react-leaflet'
import { useNavigate } from 'react-router-dom'
import type { Bounds } from '../../lib/geo'
import { mapBounds, serializeBounds } from '../../lib/geo'
import { detailStationIcon } from '../../lib/leaflet'

/// Child of <MapContainer>: turns the non-interactive preview into a link-like
/// surface. A click anywhere reports the preview's current visible bounds.
function PreviewClickHandler({ onOpen }: { onOpen: (bounds: Bounds) => void }) {
  const map = useMap()
  useMapEvents({ click: () => onOpen(mapBounds(map)) })
  return null
}

/// A small, non-interactive map preview centred on the station with a highlighted
/// marker. Clicking it opens the map view at the preview's visible bounds (a
/// history push), so the browser "back" event re-routes to this station.
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
      <MapContainer
        center={[latitude, longitude]}
        zoom={15}
        dragging={false}
        scrollWheelZoom={false}
        doubleClickZoom={false}
        zoomControl={false}
        attributionControl={false}
        className="h-full w-full cursor-pointer"
      >
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>'
          url="https://{s}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}{r}.png"
        />
        <Marker position={[latitude, longitude]} icon={detailStationIcon} alt={name} />
        <PreviewClickHandler onOpen={openMap} />
      </MapContainer>
    </div>
  )
}
