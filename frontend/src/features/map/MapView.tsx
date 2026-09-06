import { Marker, Popup } from '@vis.gl/react-maplibre'
import type { Map as MaplibreMap } from 'maplibre-gl'
import { ExternalLink } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import type { Bounds } from '../../lib/geo'
import type { StationMap } from '../stations/types'
import { stationMarkerImage } from '../../lib/map'
import {
  buildStationClusterIndex,
  clusterExpansionZoom,
  clusterMarkerKey,
  clusterStationsForView,
  isClusterFeature,
  stationFromPoint,
} from './clusterStations'
import { BaseMap } from './BaseMap'

/// The enriched identity a station popup shows, fed from the sidebar shell +
/// stats (image + description + channel count). While the shell/stats are still
/// loading the not-yet-known fields render as skeletons (same mechanism as the
/// overview) instead of popping in, so the popup does not flicker.
export interface PopupStationInfo {
  imageUrl: string
  description: string
  channelCount: number | null
}

/// The interactive MapLibre map with the visible stations. Stations that would
/// overlap at the current zoom are grouped into a numbered circle marker
/// (supercluster): clicking a circle eases the map to the zoom where it splits,
/// so the grouped stations re-render as individual flags. Clicking a flag opens
/// the station overview panel (and a popup with the station image, name,
/// description, channel count and detail link); clicking the map void closes
/// them. The marker flag is `selected` for the station whose id matches
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
  error,
  statsError,
}: {
  stations: StationMap[] | null
  initialBounds?: Bounds | null
  onBounds: (bounds: Bounds) => void
  onReady: (map: MaplibreMap) => void
  onSelectStation: (station: StationMap) => void
  onDeselect: () => void
  selectedStationId?: string | null
  stationDetails?: Map<string, PopupStationInfo>
  error?: boolean
  statsError?: boolean
}) {
  // The station whose popup is open (independent of the overview panel, which
  // the parent owns). Cleared on a map void click / station switch.
  const [popupStation, setPopupStation] = useState<StationMap | null>(null)
  const popupInfo = popupStation ? stationDetails?.get(popupStation.id) : undefined
  // The shell identity (icon + description) is still loading when there is no
  // entry for this station yet and the shell fetch has not errored.
  const shellLoading = popupInfo === undefined && !error
  // The channel-count badge is still loading while the stats sub-resource is
  // pending (the shell entry exists but its count is not known yet).
  const statsLoading = popupInfo !== undefined && popupInfo.channelCount === null && !statsError

  // The MapLibre instance this view renders into (the parent keeps its own copy
  // for the fly-to on selection). Cluster circles zoom through this instance.
  const mapRef = useRef<MaplibreMap | null>(null)
  // The current viewport, fed by BaseMap's bounds/zoom reporting: the cluster
  // circles are recomputed whenever the user pans or zooms.
  const [viewportBounds, setViewportBounds] = useState<Bounds | null>(initialBounds ?? null)
  const [zoom, setZoom] = useState<number | null>(null)

  // Rebuild the cluster index only when the station set changes (panning/
  // zooming refetches a new array per moveend, but the reference is stable in
  // between, so the index is reused across moves).
  const stationIndex = useMemo(() => buildStationClusterIndex(stations ?? []), [stations])

  // The items to render for the current view: cluster circles + ungrouped
  // station flags, each paired with a stable React key. Circle markers are
  // keyed by their sorted member station ids (see `clusterMarkerKey`) rather
  // than the Supercluster-internal cluster id, which churns when a pan/zoom
  // refetch changes the visible set — a changed key remounts the marker and
  // blinks it for a frame. Flag markers keep their station id. Empty until the
  // map has reported its first viewport.
  const viewItems = useMemo(() => {
    if (!stationIndex || !viewportBounds || zoom === null) return []
    return clusterStationsForView(stationIndex, viewportBounds, zoom).map((feature) => ({
      feature,
      key: isClusterFeature(feature)
        ? clusterMarkerKey(stationIndex, feature.properties.cluster_id)
        : stationFromPoint(feature).id,
    }))
  }, [stationIndex, viewportBounds, zoom])

  // "Click a circle to zoom into it": ease to the cluster's expansion zoom, the
  // level at which supercluster splits it into children, so the grouped
  // stations re-render as individual flags because they no longer overlap.
  const zoomToCluster = (clusterId: number, longitude: number, latitude: number) => {
    const map = mapRef.current
    if (!map || !stationIndex) return
    setPopupStation(null)
    map.easeTo({
      center: [longitude, latitude],
      zoom: clusterExpansionZoom(stationIndex, clusterId),
    })
  }

  return (
    <BaseMap
      bounds={initialBounds ?? undefined}
      onReady={(map) => {
        mapRef.current = map
        onReady(map)
      }}
      onBounds={(bounds) => {
        setViewportBounds(bounds)
        onBounds(bounds)
      }}
      onZoom={setZoom}
      navigationControl
      onVoidClick={() => {
        setPopupStation(null)
        onDeselect()
      }}
    >
      {viewItems.map(({ feature, key }) => {
        if (isClusterFeature(feature)) {
          const [longitude, latitude] = feature.geometry.coordinates
          const count = feature.properties.point_count
          return (
            <Marker key={key} longitude={longitude} latitude={latitude}>
              {/* A numbered circle instead of the stacked flags it stands for.
                  `-translate-x/y-1/2` centres it on the coordinate; the marker
                  DOM element is a child of the map container, so its click
                  bubbles up to the map's onClick — stop it here (the BaseMap
                  void-click guard ignores .maplibregl-marker clicks too). */}
              <button
                type="button"
                aria-label={`${count} stations`}
                title={`${count} stations`}
                data-count={count}
                className="station-cluster flex h-9 min-w-9 -translate-x-1/2 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border-2 border-foreground bg-primary px-1.5 text-sm leading-none font-semibold text-white shadow-lg transition-transform hover:scale-110"
                onClick={(event) => {
                  event.stopPropagation()
                  zoomToCluster(feature.properties.cluster_id, longitude, latitude)
                }}
              >
                {count}
              </button>
            </Marker>
          )
        }

        // An ungrouped station: the regular flag marker (selection/popup as
        // before). The marker DOM element is a child of the map container, so
        // its click bubbles up to the map's onClick; stop it here (the BaseMap
        // void-click guard ignores .maplibregl-marker clicks as a second layer).
        const station = stationFromPoint(feature)
        return (
          <Marker key={key} longitude={station.longitude} latitude={station.latitude}>
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
        )
      })}
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
          <div
            className="station-popup flex w-72 max-w-full flex-col gap-1.5"
            aria-busy={shellLoading || statsLoading || undefined}
          >
            {/* Icon (top left), then the heading (name) beside it, with the
                description on its own line below both. While the shell/stats
                sub-resources are still loading the not-yet-known fields render
                as skeletons (same mechanism as the overview), so the popup does
                not flicker when the data arrives. */}
            <div className="flex items-start gap-2">
              {popupInfo ? (
                <img
                  src={popupInfo.imageUrl}
                  alt=""
                  className="h-8 w-8 shrink-0 rounded border object-cover"
                />
              ) : shellLoading ? (
                <Skeleton className="h-8 w-8 shrink-0 rounded border" />
              ) : null}
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
                {popupInfo && popupInfo.channelCount !== null ? (
                  // The channel count rendered like the overview banner badge.
                  <Badge variant="secondary" className="mt-1">
                    {popupInfo.channelCount} channel
                    {popupInfo.channelCount === 1 ? '' : 's'}
                  </Badge>
                ) : statsLoading ? (
                  <Skeleton className="mt-1 h-5 w-28 rounded-md" />
                ) : shellLoading ? (
                  <Skeleton className="mt-1 h-4 w-2/3" />
                ) : null}
              </div>
            </div>
            {popupInfo ? (
              popupInfo.description && (
                <p className="break-words text-xs leading-snug text-muted-foreground">
                  {popupInfo.description}
                </p>
              )
            ) : shellLoading ? (
              <Skeleton className="h-3 w-3/4" />
            ) : null}
          </div>
        </Popup>
      )}
    </BaseMap>
  )
}
