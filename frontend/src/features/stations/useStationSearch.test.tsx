import { ok } from '@/test-utils/http'
import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationSearch } from './useStationSearch'
import type { StationSummary } from './types'

const STATIONS: StationSummary[] = [
  {
    id: 'zoo',
    name: 'Zoo Station',
    description: 'Near the zoo entrance',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 2,
    bikes_last_day: 100,
    image_url: '/img/zoo.png',
  },
  {
    id: 'alpha',
    name: 'Alpha Platz',
    description: 'Central transport hub',
    latitude: null,
    longitude: null,
    channel_count: 1,
    bikes_last_day: 5,
    image_url: '/img/alpha.png',
  },
  {
    id: 'beta',
    name: 'Beta Weg',
    description: 'A quiet residential street',
    latitude: 51.9,
    longitude: 7.6,
    channel_count: 4,
    bikes_last_day: 40,
    image_url: '/img/beta.png',
  },
]

function searchPayload(actions: Record<string, { enabled: boolean }>) {
  return { items: STATIONS, actions }
}

/// Stubs the search fetch with `actions`, renders the hook and waits until it has
/// loaded. The "filter" tests all start this way, so the setup lives here once.
async function renderLoadedSearch(actions: Record<string, { enabled: boolean }> = {}) {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(searchPayload(actions))))
  const { result } = renderHook(() => useStationSearch())
  await waitFor(() => expect(result.current.loading).toBe(false))
  return result
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationSearch', () => {
  it('loads every station and the action map, sorted by name', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(ok(searchPayload({ find_on_map: { enabled: true } }))),
    )

    const { result } = renderHook(() => useStationSearch())

    expect(result.current.loading).toBe(true)
    await waitFor(() => expect(result.current.loading).toBe(false))

    expect(result.current.error).toBe(false)
    expect(result.current.results.map((station) => station.name)).toEqual([
      'Alpha Platz',
      'Beta Weg',
      'Zoo Station',
    ])
    expect(result.current.findOnMapEnabled).toBe(true)
    expect(result.current.openDetailEnabled).toBe(false)
  })

  it('keeps the full alphabetical list when the query is empty or whitespace-only', async () => {
    const result = await renderLoadedSearch()

    act(() => result.current.setQuery('   '))
    expect(result.current.results).toHaveLength(STATIONS.length)
    expect(result.current.results[0].name).toBe('Alpha Platz')
  })

  it('filters by name case-insensitively', async () => {
    const result = await renderLoadedSearch()

    act(() => result.current.setQuery('zOO'))
    expect(result.current.results.map((station) => station.name)).toEqual(['Zoo Station'])
  })

  it('filters by description case-insensitively', async () => {
    const result = await renderLoadedSearch()

    act(() => result.current.setQuery('TRANSPORT'))
    expect(result.current.results.map((station) => station.name)).toEqual(['Alpha Platz'])
  })

  it('returns an empty result list when the query matches nothing', async () => {
    const result = await renderLoadedSearch()

    act(() => result.current.setQuery('does-not-exist'))
    expect(result.current.results).toEqual([])
  })

  it('derives the action flags from the backend action map', async () => {
    const result = await renderLoadedSearch({
      find_on_map: { enabled: true },
      open_detail: { enabled: true },
    })

    expect(result.current.findOnMapEnabled).toBe(true)
    expect(result.current.openDetailEnabled).toBe(true)
  })

  it('defaults the action flags to disabled when absent or disabled', async () => {
    const result = await renderLoadedSearch({ open_detail: { enabled: false } })

    expect(result.current.findOnMapEnabled).toBe(false)
    expect(result.current.openDetailEnabled).toBe(false)
  })

  it('reports the error state when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

    const { result } = renderHook(() => useStationSearch())
    await waitFor(() => expect(result.current.error).toBe(true))

    expect(result.current.results).toEqual([])
    expect(result.current.findOnMapEnabled).toBe(false)
    expect(result.current.openDetailEnabled).toBe(false)
  })
})
