import { useEffect, useRef, useState } from 'react'
import L from 'leaflet'
import type { Bounds } from './lib/geo'
import { escapeHtml } from './lib/format'
import type { StationSummary } from './features/stations/types'
import { useVisibleStations } from './features/stations/useVisibleStations'
import { TopBar } from './features/header/TopBar'
import { Sidebar } from './features/sidebar/Sidebar'
import { MapView } from './features/map/MapView'
import { SearchDialog } from './features/search/SearchDialog'
import { StationOverview } from './features/stationOverview/StationOverview'

/// Composition root: owns the cross-cutting state (map bounds, map instance,
/// sidebar/search visibility) and wires the feature components together.
export default function App() {
  const [bounds, setBounds] = useState<Bounds | null>(null)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  // The selected counting station (map marker click) whose overview replaces
  // the sidebar; `null` = no selection, sidebar shown.
  const [selectedStationId, setSelectedStationId] = useState<string | null>(null)

  const mapRef = useRef<L.Map | null>(null)
  const { mapStations, sidebar, error: stationsError } = useVisibleStations(bounds)

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

  return (
    <div className="flex h-screen flex-col">
      <TopBar onOpenSearch={() => setSearchOpen(true)} />

      <div className="relative flex min-h-0 flex-1">
        {selectedStationId === null ? (
          <Sidebar
            collapsed={sidebarCollapsed}
            onToggle={() => setSidebarCollapsed((collapsed) => !collapsed)}
            sidebar={sidebar}
            error={stationsError}
            onSelectStation={focusStation}
          />
        ) : (
          <StationOverview
            stationId={selectedStationId}
            onClose={() => setSelectedStationId(null)}
          />
        )}

        <main className="relative min-w-0 flex-1">
          <MapView
            stations={mapStations}
            onBounds={setBounds}
            onReady={(map) => {
              mapRef.current = map
            }}
            onSelectStation={setSelectedStationId}
            onDeselect={() => setSelectedStationId(null)}
          />
        </main>
      </div>

      {searchOpen && (
        <SearchDialog
          onClose={() => setSearchOpen(false)}
          onSelect={findAndClose}
          onFind={findAndClose}
        />
      )}
    </div>
  )
}
