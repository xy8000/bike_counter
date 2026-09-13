import { ok } from '@/test-utils/http'
import { OTHER_SUMMARY_BOUNDS, SUMMARY_BOUNDS, rawSummaryPage } from '@/test-utils/stationsSummary'
import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Bounds } from '../../lib/geo'
import { useStationsSummaryPage } from './useStationsSummaryPage'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationsSummaryPage', () => {
  it('loads and unwraps the summary shell for the given bounds', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(rawSummaryPage()))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryPage(SUMMARY_BOUNDS))

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

    const { result } = renderHook(() => useStationsSummaryPage(SUMMARY_BOUNDS))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.page).toBeNull()
  })

  it('does not fetch when the bounds are null', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(rawSummaryPage()))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryPage(null))

    expect(result.current.loading).toBe(false)
    expect(result.current.page).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refetches when the bounds change', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(rawSummaryPage()))
      .mockResolvedValueOnce(ok(rawSummaryPage()))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ bounds }: { bounds: Bounds | null }) => useStationsSummaryPage(bounds),
      { initialProps: { bounds: SUMMARY_BOUNDS } },
    )
    await waitFor(() => expect(result.current.loading).toBe(false))

    rerender({ bounds: OTHER_SUMMARY_BOUNDS })
    expect(result.current.page).toBeNull()
    expect(result.current.loading).toBe(true)

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock).toHaveBeenLastCalledWith(
      '/api/bff/stations/summary?min_lat=52&min_lng=8&max_lat=53&max_lng=9',
    )
  })
})
