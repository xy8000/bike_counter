import type { PointFeature } from 'supercluster'
import { describe, expect, it } from 'vitest'
import type { StationMap } from '../stations/types'
import {
  buildStationClusterIndex,
  clusterExpansionZoom,
  clusterMarkerKey,
  clusterStationsForView,
  isClusterFeature,
  stationFromPoint,
} from './clusterStations'
import type { StationClusterProps } from './clusterStations'

const station = (id: string, latitude: number, longitude: number): StationMap => ({
  id,
  name: id,
  latitude,
  longitude,
  status: 'active',
})

/// A viewport that contains the Münster test coordinates.
const munsterBbox = { min_lat: 51.8, min_lng: 7.4, max_lat: 52.1, max_lng: 7.8 }

describe('buildStationClusterIndex', () => {
  it('returns null when there is nothing to cluster', () => {
    expect(buildStationClusterIndex([])).toBeNull()
  })

  it('builds an index for a non-empty station set', () => {
    expect(buildStationClusterIndex([station('a', 51.96, 7.63)])).not.toBeNull()
  })
})

describe('clusterStationsForView', () => {
  it('groups stations closer than the cluster radius into a single circle', () => {
    const index = buildStationClusterIndex([
      station('a', 51.96, 7.63),
      station('b', 51.9601, 7.6301),
    ])!
    const features = clusterStationsForView(index, munsterBbox, 12)
    expect(features).toHaveLength(1)
    expect(isClusterFeature(features[0])).toBe(true)
  })

  it('returns one ungrouped point per station at the deepest zoom', () => {
    const index = buildStationClusterIndex([station('a', 51.96, 7.63), station('b', 51.9, 7.5)])!
    const features = clusterStationsForView(index, munsterBbox, 18)
    expect(features).toHaveLength(2)
    expect(features.every((feature) => !isClusterFeature(feature))).toBe(true)
  })
})

describe('clusterMarkerKey', () => {
  it('joins the sorted member ids of a cluster', () => {
    const index = buildStationClusterIndex([
      station('b', 51.96, 7.63),
      station('a', 51.9601, 7.6301),
    ])!
    const cluster = clusterStationsForView(index, munsterBbox, 10)[0] as unknown as { id: number }
    expect(clusterMarkerKey(index, cluster.id)).toBe('a|b')
  })
})

describe('clusterExpansionZoom', () => {
  it('returns a zoom at which the cluster splits', () => {
    const index = buildStationClusterIndex([
      station('a', 51.96, 7.63),
      station('b', 51.9601, 7.6301),
    ])!
    const cluster = clusterStationsForView(index, munsterBbox, 10)[0] as unknown as { id: number }
    expect(clusterExpansionZoom(index, cluster.id)).toBeGreaterThan(10)
  })
})

describe('stationFromPoint', () => {
  it('rebuilds a StationMap from an ungrouped point feature', () => {
    const index = buildStationClusterIndex([station('a', 51.96, 7.63)])!
    const points = clusterStationsForView(index, munsterBbox, 10).filter(
      (feature): feature is PointFeature<StationClusterProps> => !isClusterFeature(feature),
    )
    expect(points).toHaveLength(1)
    const rebuilt = stationFromPoint(points[0])
    expect(rebuilt.id).toBe('a')
    expect(rebuilt.name).toBe('a')
    expect(rebuilt.status).toBe('active')
    // Supercluster round-trips the coordinates through its projection, so the
    // floats are not bit-identical to the input — compare approximately.
    expect(rebuilt.latitude).toBeCloseTo(51.96, 10)
    expect(rebuilt.longitude).toBeCloseTo(7.63, 10)
  })
})
