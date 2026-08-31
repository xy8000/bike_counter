import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import type { Map as MaplibreMap } from 'maplibre-gl'
import type { Bounds } from '../../lib/geo'
import { parseBoundsQuery, serializeBounds } from '../../lib/geo'
import { useVisibleStations } from '../stations/useVisibleStations'
import { SearchableHeader } from '../header/SearchableHeader'
import { LeftPanel } from '../sidebar/LeftPanel'
import { Sidebar } from '../sidebar/Sidebar'
import { MapView, type PopupStationInfo } from './MapView'
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
/// sidebar visibility) and mirrors the visible view + open station overview into
/// the URL, so sharing a link restores them. The header/search live in the shared
/// [`SearchableHeader`].
export default function MapPage() {
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  // The visible bounding box and the open overview are seeded from the URL once;
  // the sync effect below keeps the URL in lock-step with these after that.
  const [bounds, setBounds] = useState<Bounds | null>(() => parseBoundsQuery(searchParams))
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [selectedStationId, setSelectedStationId] = useState<string | null>(
    () => searchParams.get('station') ?? null,
  )

  const mapRef = useRef<MaplibreMap | null>(null)
  const {
    mapStations,
    shell,
    stats,
    loading,
    error: stationsError,
    statsError,
  } = useVisibleStations(bounds)

  // Enriched popup identity per station, derived from the sidebar shell
  // (image + description) + the per-station stats (channel count). The map
  // popup shows the same data as the sidebar, just smaller.
  const stationDetails = useMemo(() => {
    const details = new Map<string, PopupStationInfo>()
    for (const item of shell?.items ?? []) {
      details.set(item.id, {
        imageUrl: item.image_url,
        description: item.description,
        channelCount: null,
      })
    }
    for (const [stationId, stat] of stats ?? []) {
      const entry = details.get(stationId)
      if (entry) entry.channelCount = stat.channel_count
    }
    return details
  }, [shell, stats])

  // Keyboard shortcut: H collapses/expands the sidebar (Esc is handled by the
  // searchable header, which owns the search dialog).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === 'h') {
        setSidebarCollapsed((collapsed) => !collapsed)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

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
      map.flyTo({ center: [longitude, latitude], zoom: 15, duration: 0.8 })
    }
    setSelectedStationId(id)
  }

  const findAndClose = (station: StationLocation) => {
    selectStation(station)
  }

  const openDetail = (station: StationLocation) => {
    navigate(`/stations/${station.id}`)
  }

  // "Summarize visible stations": open the summary page at the current map view
  // (the bounds are encoded so the summary restores the same selection).
  const summarizeVisible = () => {
    if (!bounds) return
    navigate(`/summary?${serializeBounds(bounds).toString()}`)
  }

  return (
    <div className="flex h-screen flex-col overflow-hidden">
      <SearchableHeader onSelect={findAndClose} onFind={findAndClose} onDetail={openDetail} />

      <div className="relative flex min-h-0 flex-1">
        {/* The generic left panel: holds either the station list or the station
            overview, and the pull/push handle collapses/expands whichever is
            currently shown. */}
        <LeftPanel
          collapsed={sidebarCollapsed}
          onToggle={() => setSidebarCollapsed((collapsed) => !collapsed)}
        >
          {selectedStationId === null ? (
            <Sidebar
              shell={shell}
              stats={stats}
              loading={loading}
              error={stationsError}
              statsError={statsError}
              onSelectStation={selectStation}
              onSummarize={summarizeVisible}
            />
          ) : (
            <StationOverview
              stationId={selectedStationId}
              onClose={() => setSelectedStationId(null)}
            />
          )}
        </LeftPanel>

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
            selectedStationId={selectedStationId}
            stationDetails={stationDetails}
          />
        </main>
      </div>
    </div>
  )
}
