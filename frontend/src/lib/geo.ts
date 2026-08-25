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
