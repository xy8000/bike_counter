import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationsSummaryGraphs } from './useStationsSummaryGraphs'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const GRAPHS = {
  current: [{ start: '2024-01-01T12:00:00.000Z', total: 5 }],
  previous: [],
  weekday_radar: [],
  weekday_radar_previous: [],
  hourly: [],
  hourly_previous: [],
  station_pie: [],
  per_station: [],
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationsSummaryGraphs', () => {
  it('loads one timeframe of the graphs card through the HATEOAS link', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(GRAPHS))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() =>
      useStationsSummaryGraphs('/api/graphs/summary/week?resolution=hour', [], true),
    )

    expect(result.current.loading).toBe(true)
    expect(result.current.graphs).toBeNull()

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe(false)
    expect(result.current.graphs?.current).toHaveLength(1)
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/graphs/summary/week?resolution=hour&exclude_new_stations=true',
    )
  })

  it('reports the error when the graphs request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() =>
      useStationsSummaryGraphs('/api/graphs/summary/week', [], false),
    )

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.graphs).toBeNull()
  })

  it('does not fetch when there is no link yet', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(GRAPHS))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryGraphs(null, [], false))

    expect(result.current.loading).toBe(false)
    expect(result.current.graphs).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refetches when the link changes (timeframe switch)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(GRAPHS))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ link }: { link: string | null }) => useStationsSummaryGraphs(link, [], false),
      { initialProps: { link: '/api/graphs/summary/week' } },
    )
    await waitFor(() => expect(result.current.graphs?.current).toHaveLength(1))

    rerender({ link: '/api/graphs/summary/year?resolution=week' })
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith('/api/graphs/summary/year?resolution=week'),
    )
  })
})
