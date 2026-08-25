import { MapContainer, Marker, Popup, TileLayer } from 'react-leaflet'
import type { Map as LeafletMap } from 'leaflet'
import type { Bounds } from '../../lib/geo'
import { MUENSTER_CENTER } from '../../lib/geo'
import type { StationMap } from '../stations/types'
import { MapController } from './MapController'
// Side effect: sets up the default marker icons and imports leaflet.css. Also
// exports the emerald stationIcon used by the markers below.
import { stationIcon } from '../../lib/leaflet'

/// The interactive Leaflet map with one marker per visible station.
export function MapView({
  stations,
  onBounds,
  onReady,
}: {
  stations: StationMap[] | null
  onBounds: (bounds: Bounds) => void
  onReady: (map: LeafletMap) => void
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
        >
          <Popup>{station.name}</Popup>
        </Marker>
      ))}
      <MapController onBounds={onBounds} onReady={onReady} />
    </MapContainer>
  )
}
