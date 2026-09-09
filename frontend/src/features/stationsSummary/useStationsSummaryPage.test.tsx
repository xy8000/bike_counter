import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Bounds } from '../../lib/geo'
import { useStationsSummaryPage } from './useStationsSummaryPage'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const BOUNDS: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }
const OTHER_BOUNDS: Bounds = { min_lat: 52, min_lng: 8, max_lat: 53, max_lng: 9 }

function rawPage() {
  return {
    image_url: '/img/summary.png',
    stations: [],
    last_update: null,
    _links: {
      self: { href: '/api/summary/self', templated: false },
      overview: { href: '/api/overview/summary', templated: false },
      graphs_day: { href: '/api/graphs/summary/day', templated: false },
      graphs_week: { href: '/api/graphs/summary/week', templated: false },
      graphs_last_30_days: { href: '/api/graphs/summary/last_30_days', templated: false },
      graphs_year: { href: '/api/graphs/summary/year', templated: false },
      monthly: { href: '/api/monthly/summary', templated: false },
    },
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationsSummaryPage', () => {
  it('loads and unwraps the summary shell for the given bounds', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(rawPage()))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryPage(BOUNDS))

    expect(result.current.loading).toBe(true)
    expect(result.current.page).toBeNull()
    expect(result.current.error).toBe(false)

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe(false)
    expect(result.current.page?._links.overview).toBe('/api/overview/summary')
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/bff/stations/summary?min_lat=51&min_lng=7&max_lat=52&max_lng=8',
    )
  })

  it('reports the error when the shell request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() => useStationsSummaryPage(BOUNDS))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.page).toBeNull()
  })

  it('does not fetch when the bounds are null', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(rawPage()))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryPage(null))

    expect(result.current.loading).toBe(false)
    expect(result.current.page).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refetches when the bounds change', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(rawPage()))
      .mockResolvedValueOnce(ok(rawPage()))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ bounds }: { bounds: Bounds | null }) => useStationsSummaryPage(bounds),
      { initialProps: { bounds: BOUNDS } },
    )
    await waitFor(() => expect(result.current.loading).toBe(false))

    rerender({ bounds: OTHER_BOUNDS })
    expect(result.current.page).toBeNull()
    expect(result.current.loading).toBe(true)

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock).toHaveBeenLastCalledWith(
      '/api/bff/stations/summary?min_lat=52&min_lng=8&max_lat=53&max_lng=9',
    )
  })
})
