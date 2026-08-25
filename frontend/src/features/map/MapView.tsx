import { MapContainer, Marker, Popup, TileLayer, useMapEvents } from 'react-leaflet'
import L from 'leaflet'
import type { Map as LeafletMap } from 'leaflet'
import type { Bounds } from '../../lib/geo'
import { MUENSTER_CENTER } from '../../lib/geo'
import type { StationMap } from '../stations/types'
import { MapController } from './MapController'
// Side effect: sets up the default marker icons and imports leaflet.css. Also
// exports the emerald stationIcon used by the markers below.
import { stationIcon } from '../../lib/leaflet'

/// Renders inside <MapContainer>: clicking the map "void" (i.e. anywhere except
/// a marker) closes the open station overview. Marker clicks stop propagation
/// (see below), so they never reach this handler. `useMapEvents` needs the
/// Leaflet map context that only <MapContainer>'s children have, so this must be
/// its own child component (not called from MapView itself).
function MapVoidClickHandler({ onDeselect }: { onDeselect: () => void }) {
  useMapEvents({ click: () => onDeselect() })
  return null
}

/// The interactive Leaflet map with one marker per visible station. Clicking a
/// marker opens the station overview panel; clicking the map void closes it.
export function MapView({
  stations,
  onBounds,
  onReady,
  onSelectStation,
  onDeselect,
}: {
  stations: StationMap[] | null
  onBounds: (bounds: Bounds) => void
  onReady: (map: LeafletMap) => void
  onSelectStation: (id: string) => void
  onDeselect: () => void
}) {
  return (
    <MapContainer center={MUENSTER_CENTER} zoom={13} className="absolute inset-0 z-0">
      {/* OpenStreetMap's public tile server (tile.openstreetmap.org) blocks
          client-side requests it can't attribute to a real app and returns
          its usage-policy 403 image instead of tiles. CARTO's free raster
          tiles permit browser use without an API key. */}
      <TileLayer
        attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>'
        url="https://{s}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}{r}.png"
      />
      {(stations ?? []).map((station) => (
        <Marker
          key={station.id}
          position={[station.latitude, station.longitude]}
          icon={stationIcon}
          // Leaflet forwards both to the marker <img>; used by the Playwright
          // e2e tests to locate a marker and to assert its popup, and improves
          // accessibility (screen readers + tooltip).
          alt={station.name}
          title={station.name}
          eventHandlers={{
            click: (event) => {
              // Do not let the marker click bubble to the map's void-click
              // handler (which would close the overview we are about to open).
              L.DomEvent.stopPropagation(event.originalEvent)
              onSelectStation(station.id)
            },
          }}
        >
          <Popup>
            <div className="flex flex-col gap-1">
              <span className="font-medium">{station.name}</span>
              {/* Link to the future detail page (rendered as an external link). */}
              <a
                href={`/stations/${station.id}`}
                target="_blank"
                rel="noreferrer"
                className="text-xs font-semibold text-primary underline-offset-2 hover:underline"
              >
                Open detail page →
              </a>
            </div>
          </Popup>
        </Marker>
      ))}
      <MapVoidClickHandler onDeselect={onDeselect} />
      <MapController onBounds={onBounds} onReady={onReady} />
    </MapContainer>
  )
}
