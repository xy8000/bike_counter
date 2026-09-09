import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchMapStations, fetchSidebarShell, fetchSidebarStats, fetchStationSearch } from './api'
import type { Bounds } from '../../lib/geo'

const BOUNDS: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }
const MARKERS_URL = '/api/bff/stations?min_lat=51&min_lng=7&max_lat=52&max_lng=8'
const SHELL_URL = '/api/bff/stations/sidebar?min_lat=51&min_lng=7&max_lat=52&max_lng=8'
const STATS_URL = '/api/bff/stations/sidebar/stats?min_lat=51&min_lng=7&max_lat=52&max_lng=8'

function okResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify(data), { status })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('fetchMapStations', () => {
  it('fetches the marker list for the given bounds and returns the items', async () => {
    const fetchMock = vi.fn().mockResolvedValue(okResponse({ items: [{ id: 'a' }, { id: 'b' }] }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchMapStations(BOUNDS)).resolves.toEqual([{ id: 'a' }, { id: 'b' }])
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith(MARKERS_URL)
  })

  it('rejects when the response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(okResponse({}, 500)))

    await expect(fetchMapStations(BOUNDS)).rejects.toThrow(`${MARKERS_URL} responded with 500`)
  })
})

describe('fetchSidebarShell', () => {
  const shellPayload = {
    items: [{ id: '1', name: 'Alpha' }],
    visible_count: 1,
    total_count: 3,
    _links: {
      stats: { href: STATS_URL, templated: false },
    },
  }

  it('fetches the shell and unwraps the stats link to its href string', async () => {
    const fetchMock = vi.fn().mockResolvedValue(okResponse(shellPayload))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchSidebarShell(BOUNDS)).resolves.toEqual({
      items: [{ id: '1', name: 'Alpha' }],
      visible_count: 1,
      total_count: 3,
      _links: { stats: STATS_URL },
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith(SHELL_URL)
  })

  it('rejects when the response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(okResponse({}, 404)))

    await expect(fetchSidebarShell(BOUNDS)).rejects.toThrow(`${SHELL_URL} responded with 404`)
  })
})

describe('fetchSidebarStats', () => {
  it('fetches the stats sub-resource through the given href', async () => {
    const statsPayload = {
      items: [{ station_id: '1', channel_count: 2, bikes_last_day: 42 }],
    }
    const fetchMock = vi.fn().mockResolvedValue(okResponse(statsPayload))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchSidebarStats(STATS_URL)).resolves.toEqual(statsPayload)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith(STATS_URL)
  })

  it('rejects when the response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(okResponse({}, 503)))

    await expect(fetchSidebarStats(STATS_URL)).rejects.toThrow(`${STATS_URL} responded with 503`)
  })
})

describe('fetchStationSearch', () => {
  it('fetches every station plus the action map', async () => {
    const searchPayload = {
      items: [{ id: '1', name: 'Alpha' }],
      actions: { find_on_map: { enabled: true } },
    }
    const fetchMock = vi.fn().mockResolvedValue(okResponse(searchPayload))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchStationSearch()).resolves.toEqual(searchPayload)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/stations/search')
  })

  it('rejects when the response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(okResponse({}, 500)))

    await expect(fetchStationSearch()).rejects.toThrow(
      '/api/bff/stations/search responded with 500',
    )
  })
})
