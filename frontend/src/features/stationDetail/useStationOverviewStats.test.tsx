import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationOverviewStats } from './useStationOverviewStats'
import type { StationOverviewStats } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const STATS: StationOverviewStats = {
  total_bikes: 1234,
  metrics: [],
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationOverviewStats', () => {
  it('fetches the overview url without the trend param when exclude is off', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(STATS))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationOverviewStats('/api/overview/s1', false))

    await waitFor(() => expect(result.current.data).toEqual(STATS))
    expect(fetchMock).toHaveBeenCalledWith('/api/overview/s1')
  })

  it('appends the trend param when exclude is on', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(STATS))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationOverviewStats('/api/overview/s1', true))

    await waitFor(() => expect(result.current.data).toEqual(STATS))
    expect(fetchMock).toHaveBeenCalledWith('/api/overview/s1?exclude_new_stations=true')
  })

  it('reports the error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() => useStationOverviewStats('/api/overview/s1', false))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.data).toBeNull()
  })
})
