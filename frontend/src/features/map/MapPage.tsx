import { useEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import type { Map as LeafletMap } from 'leaflet'
import type { Bounds } from '../../lib/geo'
import { parseBoundsQuery, serializeBounds } from '../../lib/geo'
import { useVisibleStations } from '../stations/useVisibleStations'
import { TopBar } from '../header/TopBar'
import { Sidebar } from '../sidebar/Sidebar'
import { MapView } from './MapView'
import { SearchDialog } from '../search/SearchDialog'
import { StationOverview } from '../stationOverview/StationOverview'

/// The minimal station location needed to focus the map on a selection. Both the
/// map markers (`StationMap`) and the sidebar/search summaries (`StationSummary`)
/// satisfy this shape, so a single `selectStation` handles every entry point.
interface StationLocation {
  id: string
  latitude: number | null
  longitude: number | null
}

/// The map route: owns the cross-cutting state (map bounds, map instance,
/// sidebar/search visibility) and mirrors the visible view + open station
/// overview into the URL, so sharing a link restores them.
export default function MapPage() {
  const [searchParams, setSearchParams] = useSearchParams()
  // The visible bounding box and the open overview are seeded from the URL once;
  // the sync effect below keeps the URL in lock-step with these after that.
  const [bounds, setBounds] = useState<Bounds | null>(() => parseBoundsQuery(searchParams))
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [selectedStationId, setSelectedStationId] = useState<string | null>(
    () => searchParams.get('station') ?? null,
  )

  const mapRef = useRef<LeafletMap | null>(null)
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

  // Mirror the visible bounds + open overview into the URL: panning/zooming
  // updates the bbox params, selecting/closing a station updates `station`.
  // `replace` avoids a history entry per pan and the guard skips redundant
  // writes, so there is no feedback loop with the initial URL read.
  useEffect(() => {
    const params = new URLSearchParams(searchParams)
    if (bounds) {
      serializeBounds(bounds).forEach((value, key) => params.set(key, value))
    } else {
      for (const key of ['min_lat', 'min_lng', 'max_lat', 'max_lng']) {
        params.delete(key)
      }
    }
    if (selectedStationId) {
      params.set('station', selectedStationId)
    } else {
      params.delete('station')
    }
    const next = params.toString()
    if (next !== searchParams.toString()) {
      setSearchParams(params, { replace: true })
    }
  }, [bounds, selectedStationId, searchParams, setSearchParams])

  // Single entry point for selecting a station (map marker, sidebar item or
  // search result): fly to it and open the same overview panel.
  const selectStation = ({ id, latitude, longitude }: StationLocation) => {
    const map = mapRef.current
    if (map && latitude !== null && longitude !== null) {
      map.flyTo([latitude, longitude], 15, { duration: 0.8 })
    }
    setSelectedStationId(id)
  }

  const findAndClose = (station: StationLocation) => {
    selectStation(station)
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
            onSelectStation={selectStation}
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
            initialBounds={bounds}
            onBounds={setBounds}
            onReady={(map) => {
              mapRef.current = map
            }}
            onSelectStation={selectStation}
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
