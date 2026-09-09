import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationsSummaryMonthly } from './useStationsSummaryMonthly'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const MONTHLY = {
  monthly_totals: [
    { year: 2024, month: 1, total: 12 },
    { year: 2025, month: 1, total: 20 },
  ],
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationsSummaryMonthly', () => {
  it('loads the monthly totals card through the HATEOAS link', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(MONTHLY))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() =>
      useStationsSummaryMonthly('/api/monthly/summary', ['s1'], false),
    )

    expect(result.current.loading).toBe(true)
    expect(result.current.monthly).toBeNull()

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe(false)
    expect(result.current.monthly?.monthly_totals).toHaveLength(2)
    expect(fetchMock).toHaveBeenCalledWith('/api/monthly/summary?exclude=s1')
  })

  it('reports the error when the monthly request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() =>
      useStationsSummaryMonthly('/api/monthly/summary', [], false),
    )

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.monthly).toBeNull()
  })

  it('does not fetch when there is no link yet', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(MONTHLY))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationsSummaryMonthly(null, [], false))

    expect(result.current.loading).toBe(false)
    expect(result.current.monthly).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refetches when the Bike-Trends flag changes', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(MONTHLY))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ excludeNewStations }: { excludeNewStations: boolean }) =>
        useStationsSummaryMonthly('/api/monthly/summary', [], excludeNewStations),
      { initialProps: { excludeNewStations: false } },
    )
    await waitFor(() => expect(result.current.monthly?.monthly_totals).toHaveLength(2))

    rerender({ excludeNewStations: true })
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith('/api/monthly/summary?exclude_new_stations=true'),
    )
  })
})
