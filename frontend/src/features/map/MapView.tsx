import { Marker, Popup } from '@vis.gl/react-maplibre'
import type { Map as MaplibreMap } from 'maplibre-gl'
import { ExternalLink } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router-dom'
import { Badge } from '@/components/ui/badge'
import type { Bounds } from '../../lib/geo'
import type { StationMap } from '../stations/types'
import { stationMarkerImage } from '../../lib/map'
import { BaseMap } from './BaseMap'

/// The enriched identity a station popup shows, fed from the sidebar shell +
/// stats (image + description + channel count). Falls back to the plain name +
/// detail link while the shell/stats are still loading.
export interface PopupStationInfo {
  imageUrl: string
  description: string
  channelCount: number | null
}

/// The interactive MapLibre map with one marker per visible station. Clicking a
/// marker opens the station overview panel (and a popup with the station image,
/// name, description, channel count and detail link); clicking the map void
/// closes them. The marker flag is `selected` for the station whose id matches
/// `selectedStationId` (the URL `station` param), `active`/`inactive` otherwise
/// (from the BFF-reported status). When `initialBounds` is set (from a shared
/// URL) the map fits that view on mount instead of the Münster default.
export function MapView({
  stations,
  initialBounds,
  onBounds,
  onReady,
  onSelectStation,
  onDeselect,
  selectedStationId,
  stationDetails,
}: {
  stations: StationMap[] | null
  initialBounds?: Bounds | null
  onBounds: (bounds: Bounds) => void
  onReady: (map: MaplibreMap) => void
  onSelectStation: (station: StationMap) => void
  onDeselect: () => void
  selectedStationId?: string | null
  stationDetails?: Map<string, PopupStationInfo>
}) {
  // The station whose popup is open (independent of the overview panel, which
  // the parent owns). Cleared on a map void click / station switch.
  const [popupStation, setPopupStation] = useState<StationMap | null>(null)
  const popupInfo = popupStation ? stationDetails?.get(popupStation.id) : undefined

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
            {stationMarkerImage(station.name, {
              // Inactive always wins: a decommissioned station must look
              // inactive even when it is the one selected in the URL.
              state:
                station.status === 'inactive'
                  ? 'inactive'
                  : station.id === selectedStationId
                    ? 'selected'
                    : 'active',
            })}
          </div>
        </Marker>
      ))}
      {popupStation && (
        <Popup
          longitude={popupStation.longitude}
          latitude={popupStation.latitude}
          offset={28}
          closeButton={false}
          // Override MapLibre's default popup max-width so the content lays out
          // at the width we choose instead of being squeezed (or overflowing).
          maxWidth="18rem"
        >
          <div className="station-popup flex w-72 max-w-full flex-col gap-1.5">
            {/* Icon (top left), then the heading (name) beside it, with the
                description on its own line below both. */}
            <div className="flex items-start gap-2">
              {popupInfo && (
                <img
                  src={popupInfo.imageUrl}
                  alt=""
                  className="h-8 w-8 shrink-0 rounded border object-cover"
                />
              )}
              <div className="min-w-0 flex-1">
                <div className="flex items-start justify-between gap-2">
                  {/* The station name opens the detail page in the same tab
                      without looking like a link; the icon button is the
                      explicit affordance. `min-w-0` + `break-words` keep long
                      names inside the popup. */}
                  <Link
                    to={`/stations/${popupStation.id}`}
                    className="min-w-0 break-words font-medium leading-snug text-foreground hover:no-underline"
                  >
                    {popupStation.name}
                  </Link>
                  <Link
                    to={`/stations/${popupStation.id}`}
                    aria-label="Open detail page"
                    title="Open detail page"
                    className="inline-flex shrink-0 items-center text-primary hover:underline"
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                  </Link>
                </div>
                {popupInfo && popupInfo.channelCount !== null && (
                  // The channel count rendered like the overview banner badge.
                  <Badge variant="secondary" className="mt-1">
                    {popupInfo.channelCount} channel
                    {popupInfo.channelCount === 1 ? '' : 's'}
                  </Badge>
                )}
              </div>
            </div>
            {popupInfo?.description && (
              <p className="break-words text-xs leading-snug text-muted-foreground">
                {popupInfo.description}
              </p>
            )}
          </div>
        </Popup>
      )}
    </BaseMap>
  )
}
