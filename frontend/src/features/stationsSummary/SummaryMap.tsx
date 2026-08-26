import { MapContainer, Marker, TileLayer } from 'react-leaflet'
import L from 'leaflet'
import type { Bounds } from '../../lib/geo'
import { disabledStationIcon, stationIcon } from '../../lib/leaflet'
import type { SummaryStation } from './types'

/// The interactive map on the station-summary page, fitted to the same bounds
/// the user had selected. Clicking a station's flag toggles it disabled: the
/// marker turns gray and the station is excluded from the charts (the page
/// re-fetches with the updated `disabled` set).
export function SummaryMap({
  stations,
  disabled,
  onToggle,
  bounds,
}: {
  stations: SummaryStation[]
  disabled: Set<string>
  onToggle: (stationId: string) => void
  bounds: Bounds
}) {
  return (
    <div className="h-64 w-full overflow-hidden rounded-lg border md:h-80" aria-label="Summary map">
      <MapContainer
        bounds={[
          [bounds.min_lat, bounds.min_lng],
          [bounds.max_lat, bounds.max_lng],
        ]}
        boundsOptions={{ animate: false }}
        className="h-full w-full"
        scrollWheelZoom={false}
        doubleClickZoom={false}
        zoomControl
      >
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>'
          url="https://{s}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}{r}.png"
        />
        {stations.map((station) => {
          const isDisabled = disabled.has(station.id)
          return (
            <Marker
              key={station.id}
              position={[station.latitude, station.longitude]}
              icon={isDisabled ? disabledStationIcon : stationIcon}
              // Leaflet forwards both to the marker <img>; used by the Playwright
              // e2e tests to locate a marker and to assert the disabled state.
              alt={station.name}
              title={station.name}
              eventHandlers={{
                click: (event) => {
                  // Do not let the marker click bubble to any map handler.
                  L.DomEvent.stopPropagation(event.originalEvent)
                  onToggle(station.id)
                },
              }}
            />
          )
        })}
      </MapContainer>
    </div>
  )
}
