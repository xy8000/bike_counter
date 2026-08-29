import { Marker, Popup } from '@vis.gl/react-maplibre'
import type { Map as MaplibreMap } from 'maplibre-gl'
import { ExternalLink } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router-dom'
import type { Bounds } from '../../lib/geo'
import type { StationMap } from '../stations/types'
import { stationMarkerImage } from '../../lib/map'
import { BaseMap } from './BaseMap'

/// The interactive MapLibre map with one marker per visible station. Clicking a
/// marker opens the station overview panel (and a popup with the station name +
/// detail links); clicking the map void closes them. When `initialBounds` is set
/// (from a shared URL) the map fits that view on mount instead of the Münster
/// default.
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
  onReady: (map: MaplibreMap) => void
  onSelectStation: (station: StationMap) => void
  onDeselect: () => void
}) {
  // The station whose popup is open (independent of the overview panel, which
  // the parent owns). Cleared on a map void click / station switch.
  const [popupStation, setPopupStation] = useState<StationMap | null>(null)

  return (
    <BaseMap
      bounds={initialBounds ?? undefined}
      onReady={onReady}
      onBounds={onBounds}
      navigationControl
      onVoidClick={() => {
        setPopupStation(null)
        onDeselect()
      }}
    >
      {(stations ?? []).map((station) => (
        <Marker key={station.id} longitude={station.longitude} latitude={station.latitude}>
          {/* The marker DOM element is a child of the map container, so its click
              bubbles up to the map's onClick; stop it here (the BaseMap void-click
              guard ignores .maplibregl-marker clicks as a second layer). */}
          <div
            className="cursor-pointer"
            onClick={(event) => {
              event.stopPropagation()
              setPopupStation(station)
              onSelectStation(station)
            }}
          >
            {stationMarkerImage(station.name)}
          </div>
        </Marker>
      ))}
      {popupStation && (
        <Popup
          longitude={popupStation.longitude}
          latitude={popupStation.latitude}
          offset={28}
          closeButton={false}
        >
          <div className="station-popup flex items-center gap-2">
            {/* The station name opens the detail page in the same tab without
                looking like a link; the icon button is the explicit affordance. */}
            <Link
              to={`/stations/${popupStation.id}`}
              className="font-medium text-foreground hover:no-underline"
            >
              {popupStation.name}
            </Link>
            <Link
              to={`/stations/${popupStation.id}`}
              aria-label="Open detail page"
              title="Open detail page"
              className="inline-flex items-center text-primary hover:underline"
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </Link>
          </div>
        </Popup>
      )}
    </BaseMap>
  )
}
