import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationsSummaryOverview } from './useStationsSummaryOverview'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const OVERVIEW = {
  channel_count: 5,
  total_bikes: 1234,
  metrics: [
    { key: 'last_day', current: 12, previous: 10, trend: 'up', delta_percent: 20, is_new: false },
  ],
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationsSummaryOverview', () => {
  it('loads the overview card through the HATEOAS link with the local params', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(OVERVIEW))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() =>
      useStationsSummaryOverview('/api/overview/summary', ['s1'], true),
    )

    expect(result.current.loading).toBe(true)
    expect(result.current.overview).toBeNull()

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe(false)
    expect(result.current.overview?.total_bikes).toBe(1234)
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/overview/summary?exclude=s1&exclude_new_stations=true',
    )
  })

  it('reports the error when the overview request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() =>
      useStationsSummaryOverview('/api/overview/summary', [], false),
    )

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.overview).toBeNull()
  })

  it('does not fetch when there is no link yet', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(OVERVIEW))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryOverview(null, [], false))

    expect(result.current.loading).toBe(false)
    expect(result.current.overview).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refetches when the exclude set changes', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(OVERVIEW))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ exclude }: { exclude: string[] }) =>
        useStationsSummaryOverview('/api/overview/summary', exclude, false),
      { initialProps: { exclude: [] as string[] } },
    )
    await waitFor(() => expect(result.current.overview?.total_bikes).toBe(1234))

    rerender({ exclude: ['s2'] })
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/overview/summary?exclude=s2'))
  })
})
