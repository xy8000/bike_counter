import Supercluster, {
  type AnyProps,
  type ClusterFeature,
  type InputFeature,
  type PointFeature,
} from 'supercluster'
import type { Bounds } from '../../lib/geo'
import type { StationMap, StationStatus } from '../stations/types'

/// The per-station properties carried into the cluster index — enough to rebuild
/// a `StationMap`-shaped marker (flag, popup, selection) from a point feature
/// without looking the station up again.
export type StationClusterProps = {
  id: string
  name: string
  status: StationStatus
}

/// A feature handed back by `clusterStationsForView`: either a numbered circle
/// (cluster) or one ungrouped station flag (point).
export type StationFeature = ClusterFeature<AnyProps> | PointFeature<StationClusterProps>

/// The clustering index for one fixed set of visible stations.
export type StationClusterIndex = Supercluster<StationClusterProps>

/// Grouping geometry: a pixel radius close to the 32 px flag width, so only
/// markers that actually overlap are grouped; a `maxZoom` matching the map's own
/// so near-co-located stations finally separate at the deepest zoom; and at
/// least two stations per cluster (a lone station is never a circle).
export const CLUSTER_RADIUS = 40
export const CLUSTER_MAX_ZOOM = 18
export const CLUSTER_MIN_POINTS = 2

/// Build (and load) the clustering index for the given visible stations, or
/// null when there is nothing to cluster. The index is immutable once loaded, so
/// callers memoize it per stations array and only rebuild when the set changes.
export function buildStationClusterIndex(stations: StationMap[]): StationClusterIndex | null {
  if (stations.length === 0) return null
  const index = new Supercluster<StationClusterProps>({
    radius: CLUSTER_RADIUS,
    maxZoom: CLUSTER_MAX_ZOOM,
    minPoints: CLUSTER_MIN_POINTS,
  })
  // Sort the points by a stable key (id) so the cluster ids Supercluster assigns
  // depend only on the station set, not on the order the BFF happens to return
  // them in. Identical circles then keep identical keys across refetches and are
  // reconciled instead of remounted (which would make them blink).
  const points: InputFeature<StationClusterProps>[] = stations
    .map(
      // The explicit return type keeps `type` as the literal 'Feature' (and the
      // sortable element as `InputFeature`) after the chained `.sort`.
      (station): InputFeature<StationClusterProps> => ({
        type: 'Feature',
        properties: {
          id: station.id,
          name: station.name,
          status: station.status,
        },
        geometry: { type: 'Point', coordinates: [station.longitude, station.latitude] },
      }),
    )
    .sort((a, b) => a.properties.id.localeCompare(b.properties.id))
  index.load(points)
  return index
}

/// The features to render for the current viewport: every cluster circle plus
/// every station that is not grouped. `bounds`/`zoom` are the current viewport,
/// so the circle/flag set stays aligned with what the user sees and off-screen
/// stations never render.
export function clusterStationsForView(
  index: StationClusterIndex,
  bounds: Bounds,
  zoom: number,
): StationFeature[] {
  const bbox: [number, number, number, number] = [
    bounds.min_lng,
    bounds.min_lat,
    bounds.max_lng,
    bounds.max_lat,
  ]
  return index.getClusters(bbox, zoom)
}

/// True when the feature is a numbered circle rather than an individual station.
export function isClusterFeature(feature: StationFeature): feature is ClusterFeature<AnyProps> {
  return feature.properties !== null && 'cluster' in feature.properties
}

/// The zoom at which the cluster with `clusterId` splits into several children
/// ("click to zoom": flying there un-groups the circle).
export function clusterExpansionZoom(index: StationClusterIndex, clusterId: number): number {
  return index.getClusterExpansionZoom(clusterId)
}

/// A stable React key for a cluster circle: the sorted member station ids joined
/// with '|'. Unlike the Supercluster-internal `cluster_id` — assigned by tree
/// order, so it shifts whenever a pan/zoom refetch changes the visible station
/// set — this key stays identical for a circle that keeps the same members
/// across a refetch. A stable key lets React reconcile the marker instead of
/// remounting it, which would make the circle blink for a frame.
export function clusterMarkerKey(index: StationClusterIndex, clusterId: number): string {
  return index
    .getLeaves(clusterId, Infinity)
    .map((leaf) => leaf.properties.id)
    .sort()
    .join('|')
}

/// Rebuild a `StationMap`-shaped marker from an ungrouped point feature.
export function stationFromPoint(feature: PointFeature<StationClusterProps>): StationMap {
  const [longitude, latitude] = feature.geometry.coordinates
  return {
    id: feature.properties.id,
    name: feature.properties.name,
    status: feature.properties.status,
    longitude,
    latitude,
  }
}
