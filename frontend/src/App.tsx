import { useEffect, useState } from 'react'
import L from 'leaflet'
import { MapContainer, Marker, Popup, TileLayer } from 'react-leaflet'
import iconRetinaUrl from 'leaflet/dist/images/marker-icon-2x.png'
import iconUrl from 'leaflet/dist/images/marker-icon.png'
import shadowUrl from 'leaflet/dist/images/marker-shadow.png'
import 'leaflet/dist/leaflet.css'

// Leaflet's default icon points at bare asset names; Vite bundles the images,
// so point the default icon at the resolved URLs explicitly.
L.Icon.Default.mergeOptions({
  iconRetinaUrl,
  iconUrl,
  shadowUrl,
})

interface CountingStation {
  id: string
  name: string
  latitude: number | null
  longitude: number | null
}

interface CountingStationList {
  items: CountingStation[]
}

const MUENSTER_CENTER: [number, number] = [51.96, 7.63]

export default function App() {
  const [stations, setStations] = useState<CountingStation[] | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false

    fetch('/api/v1/counting-stations')
      .then((response) => {
        if (!response.ok) {
          throw new Error(`API responded with ${response.status}`)
        }
        return response.json() as Promise<CountingStationList>
      })
      .then((data) => {
        if (!cancelled) {
          setStations(data.items)
        }
      })
      .catch(() => {
        // Keep the page useful even when the backend is unreachable.
        if (!cancelled) {
          setStations([])
          setError(true)
        }
      })

    return () => {
      cancelled = true
    }
  }, [])

  // Only stations with coordinates are plotted; "not provided" ones are skipped.
  const positioned = (stations ?? []).filter(
    (station): station is CountingStation & { latitude: number; longitude: number } =>
      station.latitude !== null && station.longitude !== null,
  )

  return (
    <main className="app">
      <h1>Bike Counter</h1>
      {error && <p className="error">Could not load counting stations.</p>}
      {stations === null && !error && <p className="bff">Loading counting stations…</p>}
      {stations !== null && (
        <MapContainer center={MUENSTER_CENTER} zoom={13} className="map">
          {/* OpenStreetMap's public tile server (tile.openstreetmap.org) blocks
              client-side requests it can't attribute to a real app and returns
              its usage-policy 403 image instead of tiles. CARTO's free raster
              tiles permit browser use without an API key. */}
          <TileLayer
            attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>'
            url="https://{s}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}{r}.png"
          />
          {positioned.map((station) => (
            <Marker
              key={station.id}
              position={[station.latitude, station.longitude]}
            >
              <Popup>{station.name}</Popup>
            </Marker>
          ))}
        </MapContainer>
      )}
    </main>
  )
}
