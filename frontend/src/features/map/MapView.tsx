import { MapContainer, Marker, Popup, TileLayer, useMapEvents } from 'react-leaflet'
import L from 'leaflet'
import type { Map as LeafletMap } from 'leaflet'
import { ExternalLink } from 'lucide-react'
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
/// When `initialBounds` is set (from a shared URL) the map fits that view on
/// mount instead of the Münster default; otherwise it keeps the default.
export function MapView({
  stations,
  initialBounds,
  onBounds,
  onReady,
  onSelectStation,
  onDeselect,
}: {
  stations: StationMap[] | null
  initialBounds?: Bounds | null
  onBounds: (bounds: Bounds) => void
  onReady: (map: LeafletMap) => void
  onSelectStation: (station: StationMap) => void
  onDeselect: () => void
}) {
  return (
    // `center`/`zoom` take priority over `bounds` in react-leaflet's
    // MapContainer, so pass either the shared bounds or the default center/zoom.
    <MapContainer
      {...(initialBounds
        ? {
            bounds: [
              [initialBounds.min_lat, initialBounds.min_lng],
              [initialBounds.max_lat, initialBounds.max_lng],
            ],
            boundsOptions: { animate: false },
          }
        : { center: MUENSTER_CENTER, zoom: 13 })}
      className="absolute inset-0 z-0"
    >
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
              onSelectStation(station)
            },
          }}
        >
          <Popup>
            <div className="flex items-center gap-2">
              {/* The station name opens the future detail page without looking
                  like a link; the icon button is the explicit affordance. */}
              <a
                href={`/stations/${station.id}`}
                target="_blank"
                rel="noreferrer"
                className="font-medium text-foreground hover:no-underline"
              >
                {station.name}
              </a>
              <a
                href={`/stations/${station.id}`}
                target="_blank"
                rel="noreferrer"
                aria-label="Open detail page"
                title="Open detail page"
                className="inline-flex items-center text-primary hover:underline"
              >
                <ExternalLink className="h-3.5 w-3.5" />
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
