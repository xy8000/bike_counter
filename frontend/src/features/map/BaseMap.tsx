import { Map, NavigationControl } from '@vis.gl/react-maplibre'
import type { Map as MaplibreMap, LngLatBoundsLike, StyleSpecification } from 'maplibre-gl'
import { useEffect, useState, type ReactNode } from 'react'
import type { Bounds } from '../../lib/geo'
import { mapBounds } from '../../lib/geo'

/// The self-hosted vector basemap style (served by nginx as a static asset).
/// Its single vector source reads `/tiles/map.pmtiles` directly via HTTP range
/// requests (the `pmtiles` protocol registered in `lib/map.tsx`) — no tile
/// server and no BFF proxy involved. A dark variant follows the OS colour
/// scheme (see `useBasemapStyle` below).
export const MAP_STYLE = '/styles/basemap.json'
export const MAP_STYLE_DARK = '/styles/basemap-dark.json'

/// Returns the basemap style URL for the current OS colour scheme. The initial
/// read is from `prefers-color-scheme` and a `change` listener re-renders when
/// the OS switches, so the map swaps basemaps automatically — there is no manual
/// theme toggle.
function useBasemapStyle(): string {
  const [dark, setDark] = useState(() => window.matchMedia('(prefers-color-scheme: dark)').matches)
  useEffect(() => {
    const query = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = (event: MediaQueryListEvent) => setDark(event.matches)
    query.addEventListener('change', onChange)
    return () => query.removeEventListener('change', onChange)
  }, [])
  return dark ? MAP_STYLE_DARK : MAP_STYLE
}

/// The default Münster view, used when a map has no explicit bounds (the map
/// starts centred on Münster, matching the previous Leaflet default).
export const DEFAULT_CENTER = { longitude: 7.63, latitude: 51.96, zoom: 13 }

/// Shared MapLibre wrapper used by all three maps. Owns the style, the container
/// sizing, attribution, keyboard zoom and the bounds/ready reporting; each map
/// adds its own markers/popup and click handling via `children`/`onVoidClick`.
export function BaseMap({
  children,
  interactive = true,
  scrollZoom = interactive,
  initialViewState,
  bounds,
  onReady,
  onBounds,
  onZoom,
  onVoidClick,
  navigationControl = false,
  navigationControlPosition = 'top-right',
}: {
  children?: ReactNode
  /// When false, drag/scroll/rotate/double-click zoom and keyboard zoom are all
  /// disabled (used by the non-interactive detail-page preview).
  interactive?: boolean
  scrollZoom?: boolean
  initialViewState?: { longitude: number; latitude: number; zoom: number }
  bounds?: Bounds
  onReady?: (map: MaplibreMap) => void
  onBounds?: (bounds: Bounds) => void
  /// Reports the current zoom whenever the view settles (map load and every
  /// moveend). Consumers (e.g. station clustering) re-derive their viewport
  /// state from this instead of reaching into the map instance.
  onZoom?: (zoom: number) => void
  onVoidClick?: (map: MaplibreMap) => void
  navigationControl?: boolean
  navigationControlPosition?: 'top-right' | 'top-left'
}) {
  const fitBounds: LngLatBoundsLike | undefined = bounds
    ? [
        [bounds.min_lng, bounds.min_lat],
        [bounds.max_lng, bounds.max_lat],
      ]
    : undefined

  // The pmtiles protocol requires a full URL with its own scheme after
  // `pmtiles://` (e.g. `pmtiles://https://host/map.pmtiles`) — a bare relative
  // path is not a valid PMTiles URL (protomaps/PMTiles#509) and silently never
  // gets past the initial TileJSON fetch. The committed style keeps a
  // placeholder so it works from any origin; it is resolved to this page's
  // origin at runtime.
  const styleUrl = useBasemapStyle()
  const [style, setStyle] = useState<StyleSpecification | string | null>(null)
  useEffect(() => {
    let cancelled = false
    fetch(styleUrl)
      .then((response) => response.json())
      .then((raw: StyleSpecification) => {
        if (cancelled) return
        for (const source of Object.values(raw.sources ?? {})) {
          if (source && source.type === 'vector' && typeof source.url === 'string') {
            source.url = source.url.replace('REPLACED_AT_RUNTIME', window.location.origin)
          }
        }
        setStyle(raw)
      })
      .catch((error) => {
        console.error('failed to load the self-hosted map style', error)
        if (!cancelled) setStyle(styleUrl)
      })
    return () => {
      cancelled = true
    }
  }, [styleUrl])

  if (style === null) return null

  return (
    <Map
      mapStyle={style}
      initialViewState={initialViewState ?? DEFAULT_CENTER}
      style={{ width: '100%', height: '100%' }}
      // Show a single world instead of wrapped copies, so a zoomed-out view does
      // not repeat continents ("Africa twice").
      renderWorldCopies={false}
      // The full zoom range: 0 shows the whole world (zoom all the way out). The
      // Germany detail in the combined pmtiles archive is built to z15 (matching
      // the source Protomaps build). Above z15 MapLibre overzooms those tiles
      // (vector fills/lines stay crisp — no new street detail appears, which
      // only comes from a higher-zoom data source).
      maxZoom={18}
      attributionControl={{ compact: true }}
      keyboard={interactive}
      dragPan={interactive}
      scrollZoom={scrollZoom}
      doubleClickZoom={interactive}
      touchZoomRotate={interactive}
      onClick={(event) => {
        // Marker/popup elements sit inside the map container, so clicking them
        // also bubbles up to the map's click handler. Those elements handle
        // their own clicks; treat only clicks that landed outside them as a
        // "void" click.
        const target = event.originalEvent.target
        if (target instanceof Element && target.closest('.maplibregl-marker, .maplibregl-popup')) {
          return
        }
        onVoidClick?.(event.target)
      }}
      onLoad={(event) => {
        const map = event.target
        // A shared/selected view is restored by fitting the map to it once the
        // style has loaded (the @vis.gl Map has no `bounds` init prop).
        if (fitBounds) map.fitBounds(fitBounds, { padding: 0, duration: 0 })
        onReady?.(map)
        onBounds?.(mapBounds(map))
        onZoom?.(map.getZoom())
      }}
      onMoveEnd={(event) => {
        const map = event.target
        onBounds?.(mapBounds(map))
        onZoom?.(map.getZoom())
      }}
    >
      {navigationControl && <NavigationControl position={navigationControlPosition} />}
      {children}
    </Map>
  )
}
