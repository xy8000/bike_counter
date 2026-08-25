import type { Map as LeafletMap } from 'leaflet'

/// Geographic bounding box used by the BFF's visible-stations query.
export interface Bounds {
  min_lat: number
  min_lng: number
  max_lat: number
  max_lng: number
}

/// The map starts centered on Münster.
export const MUENSTER_CENTER: [number, number] = [51.96, 7.63]

export function bboxQuery(bounds: Bounds): string {
  const params = new URLSearchParams({
    min_lat: String(bounds.min_lat),
    min_lng: String(bounds.min_lng),
    max_lat: String(bounds.max_lat),
    max_lng: String(bounds.max_lng),
  })
  return params.toString()
}

/// Serialize `Bounds` to the four map-view query params, rounded to 6 decimals
/// (~0.1 m) so URLs stay short and stable across pans.
export function serializeBounds(bounds: Bounds): URLSearchParams {
  const round = (value: number) => Math.round(value * 1_000_000) / 1_000_000
  return new URLSearchParams({
    min_lat: String(round(bounds.min_lat)),
    min_lng: String(round(bounds.min_lng)),
    max_lat: String(round(bounds.max_lat)),
    max_lng: String(round(bounds.max_lng)),
  })
}

/// Parse the map-view query params into a `Bounds`, or null when absent or
/// invalid (non-finite, inverted min/max or out-of-range values).
export function parseBoundsQuery(searchParams: URLSearchParams): Bounds | null {
  const min_lat = Number(searchParams.get('min_lat'))
  const min_lng = Number(searchParams.get('min_lng'))
  const max_lat = Number(searchParams.get('max_lat'))
  const max_lng = Number(searchParams.get('max_lng'))
  const valid =
    [min_lat, min_lng, max_lat, max_lng].every(Number.isFinite) &&
    min_lat < max_lat &&
    min_lng < max_lng &&
    min_lat >= -90 &&
    max_lat <= 90 &&
    min_lng >= -180 &&
    max_lng <= 180
  return valid ? { min_lat, min_lng, max_lat, max_lng } : null
}

export function mapBounds(map: LeafletMap): Bounds {
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
