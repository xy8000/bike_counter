import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationMonthly, type MonthlyTotals } from './useStationMonthly'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const MONTHLY: MonthlyTotals = {
  monthly_totals: [
    { year: 2025, month: 1, total: 10 },
    { year: 2025, month: 2, total: 20 },
  ],
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationMonthly', () => {
  it('fetches the monthly totals from the given url', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(MONTHLY))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationMonthly('/api/monthly/s1'))

    await waitFor(() => expect(result.current.data).toEqual(MONTHLY))
    expect(result.current.error).toBe(false)
    expect(fetchMock).toHaveBeenCalledWith('/api/monthly/s1')
  })

  it('reports the error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() => useStationMonthly('/api/monthly/s1'))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.data).toBeNull()
  })

  it('does not fetch when the url is null', () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationMonthly(null))

    expect(result.current.data).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
