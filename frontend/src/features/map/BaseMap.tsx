import { Map, NavigationControl } from '@vis.gl/react-maplibre'
import type { Map as MaplibreMap, LngLatBoundsLike, StyleSpecification } from 'maplibre-gl'
import { useEffect, useState, type ReactNode } from 'react'
import type { Bounds } from '../../lib/geo'
import { mapBounds } from '../../lib/geo'

/// The self-hosted vector basemap style (served by nginx as a static asset).
/// Its `tiles` point at the BFF map-tile proxy `/api/map/{z}/{x}/{y}`
/// (Frontend -> BFF -> Martin).
export const MAP_STYLE = '/styles/basemap.json'

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

  // MapLibre cannot construct a `Request` from a relative tile URL, so the
  // style's `tiles` are resolved to absolute URLs against the page origin at
  // runtime (the committed style keeps them relative, so any origin works).
  const [style, setStyle] = useState<StyleSpecification | string | null>(null)
  useEffect(() => {
    let cancelled = false
    fetch(MAP_STYLE)
      .then((response) => response.json())
      .then((raw: StyleSpecification) => {
        if (cancelled) return
        // MapLibre cannot build a `Request` from a relative tile URL, so every
        // vector source's `tiles` are resolved to absolute URLs against the page
        // origin at runtime (the committed style keeps them relative, so any
        // origin works). Prepend the origin via string concatenation (NOT
        // `new URL`, which percent-encodes the `{z}/{x}/{y}` placeholders).
        for (const source of Object.values(raw.sources ?? {})) {
          if (source && source.type === 'vector' && Array.isArray(source.tiles)) {
            source.tiles = source.tiles.map((tile) =>
              tile.startsWith('/') ? `${window.location.origin}${tile}` : tile,
            )
          }
        }
        setStyle(raw)
      })
      .catch((error) => {
        console.error('failed to load the self-hosted map style', error)
        if (!cancelled) setStyle(MAP_STYLE)
      })
    return () => {
      cancelled = true
    }
  }, [])

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
      // Germany `basemap` source is built to z14, but MapLibre overzooms those
      // tiles up to z15 (one extra "street" zoom level) with no additional layer
      // or data. The `world` source (Natural Earth) caps at z5; the `basemap`
      // source (Germany OSM) runs from z5 to z14, so Germany takes over exactly
      // where the world backdrop ends.
      maxZoom={15}
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
        if (
          target instanceof Element &&
          target.closest('.maplibregl-marker, .maplibregl-popup')
        ) {
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
      }}
      onMoveEnd={(event) => onBounds?.(mapBounds(event.target))}
    >
      {navigationControl && <NavigationControl position={navigationControlPosition} />}
      {children}
    </Map>
  )
}
