import { useEffect, useRef, useState } from 'react'
import L from 'leaflet'
import {
  MapContainer,
  Marker,
  Popup,
  TileLayer,
  useMap,
  useMapEvents,
} from 'react-leaflet'
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

/// Minimal map marker: only what the map needs (from GET /api/bff/stations).
interface StationMap {
  id: string
  name: string
  latitude: number
  longitude: number
}

interface StationMapList {
  items: StationMap[]
}

/// A station summary (from GET /api/bff/stations/sidebar and .../search).
interface StationSummary {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  channel_count: number
  bikes_last_24h: number
}

/// Sidebar payload: summaries + the visible/global counter.
interface StationSummarySidebar {
  items: StationSummary[]
  visible_count: number
  total_count: number
}

/// A possible action offered by the search dialog (from .../search).
interface ActionDto {
  enabled: boolean
}

/// Search payload: every station plus the map of possible actions.
interface StationSearch {
  items: StationSummary[]
  actions: Record<string, ActionDto>
}

/// Whole-system statistics for the header (from GET /api/bff/global-summary).
interface GlobalSummary {
  station_count: number
  channel_count: number
  bikes_last_24h_total: number
  last_update: string | null
}

interface Bounds {
  min_lat: number
  min_lng: number
  max_lat: number
  max_lng: number
}

const MUENSTER_CENTER: [number, number] = [51.96, 7.63]

function bboxQuery(bounds: Bounds): string {
  const params = new URLSearchParams({
    min_lat: String(bounds.min_lat),
    min_lng: String(bounds.min_lng),
    max_lat: String(bounds.max_lat),
    max_lng: String(bounds.max_lng),
  })
  return params.toString()
}

function mapBounds(map: L.Map): Bounds {
  const bounds = map.getBounds()
  const southWest = bounds.getSouthWest()
  const northEast = bounds.getNorthEast()
  return {
    min_lat: southWest.lat,
    min_lng: southWest.lng,
    max_lat: northEast.lat,
    max_lng: northEast.lng,
  }
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat('de-DE').format(value)
}

function formatTimestamp(iso: string | null): string {
  if (!iso) return 'never'
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return 'never'
  return date.toLocaleString('de-DE', { dateStyle: 'short', timeStyle: 'short' })
}

function escapeHtml(value: string): string {
  const ampersand = String.fromCharCode(38)
  return value.replace(/[&<>"']/g, (char) => {
    const replacements: Record<string, string> = {
      '&': ampersand + 'amp;',
      '<': String.fromCharCode(60) + 'lt;',
      '>': String.fromCharCode(62) + 'gt;',
      '"': String.fromCharCode(34) + 'quot;',
      "'": String.fromCharCode(39) + '#39;',
    }
    return replacements[char] ?? char
  })
}

/// A shared list entry used by both the sidebar and the search dialog. When
/// `showFind` is set, a "find on map" button is rendered next to the entry.
function StationListItem({
  station,
  onSelect,
  onFind,
  showFind,
}: {
  station: StationSummary
  onSelect: (station: StationSummary) => void
  onFind?: (station: StationSummary) => void
  showFind: boolean
}) {
  const findable = station.latitude !== null && station.longitude !== null
  return (
    <li className="station-item">
      <button type="button" className="station-item-main" onClick={() => onSelect(station)}>
        <span className="station-name">{station.name}</span>
        <span className="station-description">{station.description}</span>
        <span className="station-meta">
          {station.channel_count} channels · {formatNumber(station.bikes_last_24h)} bikes / 24 h
        </span>
      </button>
      {showFind && onFind && findable && (
        <button type="button" className="station-find" onClick={() => onFind(station)}>
          Find on map
        </button>
      )}
    </li>
  )
}

/// Bridges Leaflet's map events to React state: reports the current bounding
/// box on mount and on every `moveend`, and hands the map instance to the app.
function MapController({
  onBounds,
  onReady,
}: {
  onBounds: (bounds: Bounds) => void
  onReady: (map: L.Map) => void
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

export default function App() {
  const [mapStations, setMapStations] = useState<StationMap[] | null>(null)
  const [sidebar, setSidebar] = useState<StationSummarySidebar | null>(null)
  const [stationsError, setStationsError] = useState(false)
  const [bounds, setBounds] = useState<Bounds | null>(null)

  const [globalSummary, setGlobalSummary] = useState<GlobalSummary | null>(null)
  const [globalSummaryError, setGlobalSummaryError] = useState(false)

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [allStations, setAllStations] = useState<StationSummary[] | null>(null)
  const [actions, setActions] = useState<Record<string, ActionDto>>({})
  const [allStationsError, setAllStationsError] = useState(false)

  const mapRef = useRef<L.Map | null>(null)

  // Fetch the visible stations (map markers) + sidebar summaries whenever the
  // map bounds change, debounced so panning doesn't hammer the BFF.
  useEffect(() => {
    if (!bounds) return
    const timer = setTimeout(() => {
      const query = bboxQuery(bounds)
      Promise.all([
        fetch(`/api/bff/stations?${query}`).then((response) => {
          if (!response.ok) throw new Error(`stations responded with ${response.status}`)
          return response.json() as Promise<StationMapList>
        }),
        fetch(`/api/bff/stations/sidebar?${query}`).then((response) => {
          if (!response.ok) throw new Error(`sidebar responded with ${response.status}`)
          return response.json() as Promise<StationSummarySidebar>
        }),
      ])
        .then(([mapData, sidebarData]) => {
          setMapStations(mapData.items)
          setSidebar(sidebarData)
          setStationsError(false)
        })
        .catch(() => {
          setStationsError(true)
        })
    }, 250)
    return () => clearTimeout(timer)
  }, [bounds])

  // Load the whole-system summary (header) once, on mount.
  useEffect(() => {
    setGlobalSummaryError(false)
    fetch('/api/bff/global-summary')
      .then((response) => {
        if (!response.ok) throw new Error(`global summary responded with ${response.status}`)
        return response.json() as Promise<GlobalSummary>
      })
      .then((data) => setGlobalSummary(data))
      .catch(() => setGlobalSummaryError(true))
  }, [])

  // Load every station + the action map (no bounds) once the search opens.
  useEffect(() => {
    if (!searchOpen) return
    setAllStations(null)
    setAllStationsError(false)
    fetch('/api/bff/stations/search')
      .then((response) => {
        if (!response.ok) throw new Error(`search responded with ${response.status}`)
        return response.json() as Promise<StationSearch>
      })
      .then((data) => {
        setAllStations(data.items)
        setActions(data.actions)
      })
      .catch(() => setAllStationsError(true))
  }, [searchOpen])

  // Keyboard shortcuts: H collapses/expands the sidebar, Esc closes the search.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === 'h') {
        setSidebarCollapsed((collapsed) => !collapsed)
      } else if (event.key === 'Escape' && searchOpen) {
        setSearchOpen(false)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [searchOpen])

  const focusStation = (station: StationSummary) => {
    const map = mapRef.current
    if (!map || station.latitude === null || station.longitude === null) return
    const latitude = station.latitude
    const longitude = station.longitude
    map.flyTo([latitude, longitude], 15, { duration: 0.8 })
    // Wait for the fly animation to finish, then open a popup that mirrors the
    // map markers: plain station name, anchored above the marker (using the
    // default marker icon's popup anchor).
    map.once('moveend', () => {
      const popupAnchor = new L.Icon.Default().options.popupAnchor
      L.popup({ offset: popupAnchor })
        .setLatLng([latitude, longitude])
        .setContent(escapeHtml(station.name))
        .openOn(map)
    })
  }

  const findAndClose = (station: StationSummary) => {
    focusStation(station)
    setSearchOpen(false)
  }

  const filteredAll = (allStations ?? []).filter((station) => {
    const query = searchQuery.trim().toLowerCase()
    if (!query) return true
    return (
      station.name.toLowerCase().includes(query) ||
      station.description.toLowerCase().includes(query)
    )
  })

  // The find-on-map action comes from the BFF action map; for now it is always
  // enabled, but the UI renders it based on the backend contract.
  const findOnMapEnabled = actions.find_on_map?.enabled ?? false

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            🚴
          </span>
          <span className="brand-name">Bike Counter</span>
        </div>

        <button type="button" className="search-trigger" onClick={() => setSearchOpen(true)}>
          <span aria-hidden="true">🔍</span> Search counting stations…
        </button>

        <div className="topbar-right">
          {globalSummary && (
            <span className="global-summary">
              {globalSummary.station_count} stations · {formatNumber(globalSummary.channel_count)}{' '}
              channels · {formatNumber(globalSummary.bikes_last_24h_total)} bikes / 24 h · updated{' '}
              {formatTimestamp(globalSummary.last_update)}
            </span>
          )}
          {globalSummaryError && <span className="global-summary error">Global summary unavailable.</span>}
        </div>
      </header>

      <div className="workspace">
        <aside className={`sidebar${sidebarCollapsed ? ' collapsed' : ''}`}>
          {sidebarCollapsed ? (
            <button
              type="button"
              className="sidebar-edge"
              onClick={() => setSidebarCollapsed(false)}
              title="Show station list (H)"
              aria-label="Show station list"
            >
              {'>'}
            </button>
          ) : (
            <>
              <div className="sidebar-header">
                <h2>Visible counting stations</h2>
                <div className="sidebar-header-actions">
                  <span className="count-badge">
                    {sidebar ? `${sidebar.visible_count} / ${sidebar.total_count}` : '–'}
                  </span>
                  <button
                    type="button"
                    className="collapse-toggle"
                    onClick={() => setSidebarCollapsed(true)}
                    title="Hide station list (H)"
                    aria-label="Hide station list"
                  >
                    {'<'}
                  </button>
                </div>
              </div>
              <ul className="station-list">
                {stationsError && <li className="state error">Could not load counting stations.</li>}
                {!stationsError && sidebar === null && (
                  <li className="state">Loading counting stations…</li>
                )}
                {!stationsError && sidebar !== null && sidebar.items.length === 0 && (
                  <li className="state">No counting stations visible in this area.</li>
                )}
                {!stationsError &&
                  (sidebar?.items ?? []).map((station) => (
                    <StationListItem
                      key={station.id}
                      station={station}
                      onSelect={focusStation}
                      showFind={false}
                    />
                  ))}
              </ul>
            </>
          )}
        </aside>

        <main className="map-area">
          <MapContainer center={MUENSTER_CENTER} zoom={13} className="map">
            {/* OpenStreetMap's public tile server (tile.openstreetmap.org) blocks
                client-side requests it can't attribute to a real app and returns
                its usage-policy 403 image instead of tiles. CARTO's free raster
                tiles permit browser use without an API key. */}
            <TileLayer
              attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>'
              url="https://{s}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}{r}.png"
            />
            {(mapStations ?? []).map((station) => (
              <Marker key={station.id} position={[station.latitude, station.longitude]}>
                <Popup>{station.name}</Popup>
              </Marker>
            ))}
            <MapController
              onBounds={setBounds}
              onReady={(map) => {
                mapRef.current = map
              }}
            />
          </MapContainer>
        </main>
      </div>

      {searchOpen && (
        <div className="dialog-overlay" onClick={() => setSearchOpen(false)}>
          <div
            className="dialog"
            role="dialog"
            aria-modal="true"
            aria-label="Search counting stations"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="dialog-header">
              <input
                autoFocus
                type="text"
                placeholder="Filter stations by name or description…"
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
              />
              {searchQuery !== '' && (
                <button
                  type="button"
                  className="dialog-clear"
                  onClick={() => setSearchQuery('')}
                  aria-label="Clear filter"
                  title="Clear filter"
                >
                  ✕
                </button>
              )}
              <button
                type="button"
                className="dialog-close"
                onClick={() => setSearchOpen(false)}
                aria-label="Close search"
              >
                Close
              </button>
            </div>
            <ul className="station-list">
              {allStationsError && <li className="state error">Could not load stations.</li>}
              {!allStationsError && allStations === null && (
                <li className="state">Loading stations…</li>
              )}
              {!allStationsError && allStations !== null && filteredAll.length === 0 && (
                <li className="state">No stations match your search.</li>
              )}
              {!allStationsError &&
                filteredAll.map((station) => (
                  <StationListItem
                    key={station.id}
                    station={station}
                    onSelect={findAndClose}
                    onFind={findAndClose}
                    showFind={findOnMapEnabled}
                  />
                ))}
            </ul>
          </div>
        </div>
      )}
    </div>
  )
}
