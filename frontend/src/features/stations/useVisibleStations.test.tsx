import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Bounds } from '../../lib/geo'
import type { SidebarShell, SidebarStation, SidebarStationStats, StationMap } from './types'
import { useVisibleStations } from './useVisibleStations'

const BOUNDS_A: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }
const BOUNDS_B: Bounds = { min_lat: 51.1, min_lng: 7.1, max_lat: 52.1, max_lng: 8.1 }
const MARKERS_URL = '/api/bff/stations?min_lat=51&min_lng=7&max_lat=52&max_lng=8'
const SHELL_URL = '/api/bff/stations/sidebar?min_lat=51&min_lng=7&max_lat=52&max_lng=8'
const STATS_URL = '/api/bff/stations/sidebar/stats?min_lat=51&min_lng=7&max_lat=52&max_lng=8'

const MARKERS: StationMap[] = [
  { id: '1', name: 'Alpha', status: 'active', latitude: 51.9, longitude: 7.6 },
  { id: '2', name: 'Beta', status: 'active', latitude: 51.95, longitude: 7.65 },
]
const SHELL_ITEMS: SidebarStation[] = [
  {
    id: '1',
    name: 'Alpha',
    description: 'Alpha station',
    latitude: 51.9,
    longitude: 7.6,
    image_url: '/img/a.png',
  },
  {
    id: '2',
    name: 'Beta',
    description: 'Beta station',
    latitude: 51.95,
    longitude: 7.65,
    image_url: '/img/b.png',
  },
]
const STATS_ITEMS: SidebarStationStats[] = [
  { station_id: '1', channel_count: 2, bikes_last_day: 100 },
  { station_id: '2', channel_count: 1, bikes_last_day: 5 },
]

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

/// The raw wire shape the BFF returns (its `_links.stats` is a `LinkDto`, which
/// `fetchSidebarShell` unwraps to a plain `href` string).
type RawShell = Omit<SidebarShell, '_links'> & {
  _links: { stats: { href: string; templated?: boolean } }
}

function shellPayload(items: SidebarStation[]): RawShell {
  return {
    items,
    visible_count: items.length,
    total_count: 5,
    _links: { stats: { href: STATS_URL, templated: false } },
  }
}

interface ServerOptions {
  markers?: StationMap[]
  shellItems?: SidebarStation[]
  statsItems?: SidebarStationStats[]
  mapError?: boolean
  shellError?: boolean
  statsError?: boolean
}

function makeServer(options: ServerOptions = {}) {
  const state = {
    markers: options.markers ?? MARKERS,
    shellItems: options.shellItems ?? SHELL_ITEMS,
    statsItems: options.statsItems ?? STATS_ITEMS,
    mapError: options.mapError ?? false,
    shellError: options.shellError ?? false,
    statsError: options.statsError ?? false,
  }
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (url.includes('/sidebar/stats')) {
      if (state.statsError) return Promise.reject(new Error('stats failed'))
      return Promise.resolve(ok({ items: state.statsItems }))
    }
    if (url.includes('/stations/sidebar')) {
      if (state.shellError) return Promise.reject(new Error('shell failed'))
      return Promise.resolve(ok(shellPayload(state.shellItems)))
    }
    if (url.includes('/stations?')) {
      if (state.mapError) return Promise.reject(new Error('map failed'))
      return Promise.resolve(ok({ items: state.markers }))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return { mock, state }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('useVisibleStations', () => {
  it('does nothing and fetches nothing while the bounds are null', () => {
    const server = makeServer()
    const { result } = renderHook(() => useVisibleStations(null))

    expect(result.current).toEqual({
      mapStations: null,
      shell: null,
      stats: null,
      loading: false,
      error: false,
      statsError: false,
    })

    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(server.mock).not.toHaveBeenCalled()
  })

  it('debounces, then fetches markers + shell and finally the stats sub-resource', async () => {
    const server = makeServer()
    const { result } = renderHook(() => useVisibleStations(BOUNDS_A))

    expect(result.current.loading).toBe(false)
    expect(server.mock).not.toHaveBeenCalled()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })

    const calls = server.mock.mock.calls.map(([url]) => url)
    expect(calls).toEqual([MARKERS_URL, SHELL_URL, STATS_URL])
    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBe(false)
    expect(result.current.statsError).toBe(false)
    expect(result.current.mapStations).toEqual(MARKERS)
    expect(result.current.shell).toEqual({
      items: SHELL_ITEMS,
      visible_count: 2,
      total_count: 5,
      _links: { stats: STATS_URL },
    })
    expect(result.current.stats).toEqual(
      new Map([
        ['1', STATS_ITEMS[0]],
        ['2', STATS_ITEMS[1]],
      ]),
    )
  })

  it('drives the loading flag while the first request is in flight', async () => {
    const server = makeServer()
    const { result } = renderHook(() => useVisibleStations(BOUNDS_A))

    act(() => {
      vi.advanceTimersByTime(250)
    })
    // The debounce fired and the fetches started, but their promises have not
    // resolved yet, so the skeleton-driving loading flag is up.
    expect(result.current.loading).toBe(true)
    expect(server.mock).toHaveBeenCalledTimes(2)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(result.current.loading).toBe(false)
    expect(result.current.mapStations).toEqual(MARKERS)
    expect(result.current.shell).not.toBeNull()
  })

  it('keeps the previous map markers reference when a refetch returns the same set', async () => {
    const server = makeServer()
    const { result, rerender } = renderHook(
      (props: { bounds: Bounds }) => useVisibleStations(props.bounds),
      { initialProps: { bounds: BOUNDS_A } },
    )

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })
    const firstMarkers = result.current.mapStations
    expect(firstMarkers).toEqual(MARKERS)

    // Zoom around an unchanged view: the BFF returns the same stations, just in
    // a different order, so the hook must not replace the markers array.
    server.state.markers = [MARKERS[1], MARKERS[0]]
    rerender({ bounds: BOUNDS_B })

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })

    expect(result.current.mapStations).toBe(firstMarkers)
    expect(result.current.error).toBe(false)
  })

  it('replaces the map markers when a refetch returns a different set', async () => {
    const server = makeServer()
    const { result, rerender } = renderHook(
      (props: { bounds: Bounds }) => useVisibleStations(props.bounds),
      { initialProps: { bounds: BOUNDS_A } },
    )

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })
    const firstMarkers = result.current.mapStations

    server.state.markers = []
    rerender({ bounds: BOUNDS_B })

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })

    expect(result.current.mapStations).not.toBe(firstMarkers)
    expect(result.current.mapStations).toEqual([])
  })

  it('reports the error state when the shell fetch rejects', async () => {
    const server = makeServer({ shellError: true })
    const { result } = renderHook(() => useVisibleStations(BOUNDS_A))

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })

    expect(server.mock).toHaveBeenCalledTimes(2)
    expect(result.current.error).toBe(true)
    expect(result.current.loading).toBe(false)
    expect(result.current.shell).toBeNull()
    expect(result.current.mapStations).toBeNull()
    expect(result.current.stats).toBeNull()
    expect(result.current.statsError).toBe(false)
  })

  it('reports the error state when the map-marker fetch rejects', async () => {
    makeServer({ mapError: true })
    const { result } = renderHook(() => useVisibleStations(BOUNDS_A))

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })

    expect(result.current.error).toBe(true)
    expect(result.current.loading).toBe(false)
  })

  it('keeps shell data and reports statsError when only the stats fetch rejects', async () => {
    const server = makeServer({ statsError: true })
    const { result } = renderHook(() => useVisibleStations(BOUNDS_A))

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250)
    })

    expect(server.mock).toHaveBeenCalledTimes(3)
    expect(result.current.statsError).toBe(true)
    expect(result.current.error).toBe(false)
    expect(result.current.loading).toBe(false)
    expect(result.current.shell).not.toBeNull()
    expect(result.current.mapStations).toEqual(MARKERS)
    expect(result.current.stats).toBeNull()
  })

  it('clears the debounce timer on unmount so nothing is fetched', () => {
    const server = makeServer()
    const { unmount } = renderHook(() => useVisibleStations(BOUNDS_A))

    unmount()
    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(server.mock).not.toHaveBeenCalled()
  })

  it('cancels an in-flight request on unmount so the stats sub-resource is never fetched', async () => {
    const server = makeServer()
    const { result, unmount } = renderHook(() => useVisibleStations(BOUNDS_A))

    act(() => {
      vi.advanceTimersByTime(250)
    })
    expect(server.mock).toHaveBeenCalledTimes(2)

    unmount()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0)
    })

    expect(server.mock).toHaveBeenCalledTimes(2)
    expect(result.current.error).toBe(false)
    expect(result.current.statsError).toBe(false)
  })
})
