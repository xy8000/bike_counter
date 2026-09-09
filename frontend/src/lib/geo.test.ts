import type { Map as MaplibreMap } from 'maplibre-gl'
import { describe, expect, it } from 'vitest'
import {
  bboxQuery,
  mapBounds,
  parseBoundsQuery,
  parseDisabled,
  serializeBounds,
  stationBounds,
} from './geo'

describe('bboxQuery', () => {
  it('serializes the four corners into query params', () => {
    const bounds = { min_lat: 1, min_lng: 2, max_lat: 3, max_lng: 4 }
    expect(bboxQuery(bounds)).toBe('min_lat=1&min_lng=2&max_lat=3&max_lng=4')
  })
})

describe('serializeBounds', () => {
  it('rounds coordinates to 6 decimals to keep URLs short and stable', () => {
    const params = serializeBounds({
      min_lat: 51.1234567,
      min_lng: 7.11111111,
      max_lat: 51.99999999,
      max_lng: 7.99999999,
    })
    expect(params.get('min_lat')).toBe('51.123457')
    expect(params.get('min_lng')).toBe('7.111111')
    expect(params.get('max_lat')).toBe('52')
    expect(params.get('max_lng')).toBe('8')
  })
})

describe('stationBounds', () => {
  it('centres a default span around the station', () => {
    const bounds = stationBounds(51.96, 7.63)
    expect(bounds.min_lat).toBeCloseTo(51.956, 10)
    expect(bounds.min_lng).toBeCloseTo(7.626, 10)
    expect(bounds.max_lat).toBeCloseTo(51.964, 10)
    expect(bounds.max_lng).toBeCloseTo(7.634, 10)
  })

  it('accepts a custom half-width span', () => {
    expect(stationBounds(0, 0, 0.01)).toEqual({
      min_lat: -0.01,
      min_lng: -0.01,
      max_lat: 0.01,
      max_lng: 0.01,
    })
  })
})

describe('parseBoundsQuery', () => {
  it('parses a valid bounds query', () => {
    const params = new URLSearchParams('min_lat=51.9&min_lng=7.6&max_lat=51.99&max_lng=7.7')
    expect(parseBoundsQuery(params)).toEqual({
      min_lat: 51.9,
      min_lng: 7.6,
      max_lat: 51.99,
      max_lng: 7.7,
    })
  })

  it('returns null when the params are missing', () => {
    expect(parseBoundsQuery(new URLSearchParams())).toBeNull()
  })

  it('returns null for non-finite values', () => {
    expect(
      parseBoundsQuery(new URLSearchParams('min_lat=abc&min_lng=7&max_lat=8&max_lng=9')),
    ).toBeNull()
  })

  it('returns null for inverted ranges', () => {
    const params = new URLSearchParams('min_lat=52&min_lng=8&max_lat=51&max_lng=7')
    expect(parseBoundsQuery(params)).toBeNull()
  })

  it('returns null for out-of-range latitudes', () => {
    const params = new URLSearchParams('min_lat=-95&min_lng=-170&max_lat=-94&max_lng=-169')
    expect(parseBoundsQuery(params)).toBeNull()
  })
})

describe('parseDisabled', () => {
  it('returns an empty list when the param is absent', () => {
    expect(parseDisabled(new URLSearchParams())).toEqual([])
  })

  it('splits, trims and drops empty entries of the CSV', () => {
    const params = new URLSearchParams('disabled=%201%20,2, ,3%20')
    expect(parseDisabled(params)).toEqual(['1', '2', '3'])
  })
})

describe('mapBounds', () => {
  it('reads the four edges from a map', () => {
    const map = {
      getBounds: () => ({
        getSouth: () => 1,
        getWest: () => 2,
        getNorth: () => 3,
        getEast: () => 4,
      }),
    } as unknown as MaplibreMap
    expect(mapBounds(map)).toEqual({ min_lat: 1, min_lng: 2, max_lat: 3, max_lng: 4 })
  })
})
