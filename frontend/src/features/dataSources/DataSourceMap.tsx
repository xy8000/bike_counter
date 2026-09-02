import { Marker } from '@vis.gl/react-maplibre'
import type { Bounds } from '../../lib/geo'
import { stationMarkerImage } from '../../lib/map'
import { BaseMap } from '../map/BaseMap'
import type { DataSourceMapStation } from './types'

/// A bounding box covering every positioned station of the data source, padded
/// so the edge markers are not clipped. Falls back to null when there is no
/// station (the caller shows a placeholder instead of a map).
function boundsOf(stations: DataSourceMapStation[]): Bounds | null {
  if (stations.length === 0) return null
  let minLat = Infinity
  let minLng = Infinity
  let maxLat = -Infinity
  let maxLng = -Infinity
  for (const station of stations) {
    minLat = Math.min(minLat, station.latitude)
    minLng = Math.min(minLng, station.longitude)
    maxLat = Math.max(maxLat, station.latitude)
    maxLng = Math.max(maxLng, station.longitude)
  }
  const padLat = Math.max((maxLat - minLat) * 0.15, 0.01)
  const padLng = Math.max((maxLng - minLng) * 0.15, 0.01)
  return {
    min_lat: minLat - padLat,
    min_lng: minLng - padLng,
    max_lat: maxLat + padLat,
    max_lng: maxLng + padLng,
  }
}

/// A non-interactive MapLibre preview showing every station the data source
/// provides, each with its regular map flag (active/inactive).
export function DataSourceMap({
  stations,
  name,
}: {
  stations: DataSourceMapStation[]
  name: string
}) {
  const bounds = boundsOf(stations)

  if (bounds === null) {
    return (
      <div className="flex h-64 w-full items-center justify-center rounded-lg border bg-muted p-4 text-center text-sm text-muted-foreground md:h-80">
        No positioned stations available for this data source.
      </div>
    )
  }

  return (
    <div
      className="h-64 w-full overflow-hidden rounded-lg border md:h-80"
      aria-label={`${name} stations map`}
    >
      <BaseMap bounds={bounds} scrollZoom={false} navigationControl>
        {stations.map((station) => (
          <Marker key={station.id} longitude={station.longitude} latitude={station.latitude}>
            {stationMarkerImage(station.name, {
              state: station.status === 'inactive' ? 'inactive' : 'active',
            })}
          </Marker>
        ))}
      </BaseMap>
    </div>
  )
}
