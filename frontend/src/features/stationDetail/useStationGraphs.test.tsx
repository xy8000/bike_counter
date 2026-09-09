import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationGraphs } from './useStationGraphs'
import type { PeriodGraphs } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const GRAPHS: PeriodGraphs = {
  current: [],
  previous: [],
  weekday_radar: [],
  weekday_radar_previous: [],
  hourly: [],
  hourly_previous: [],
  channel_pie: [],
  per_channel: [],
  is_new: false,
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationGraphs', () => {
  it('fetches the graphs url without the trend param when exclude is off', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(GRAPHS))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationGraphs('/api/graphs/s1/week', false))

    await waitFor(() => expect(result.current.data).toEqual(GRAPHS))
    expect(fetchMock).toHaveBeenCalledWith('/api/graphs/s1/week')
  })

  it('appends the trend param when exclude is on', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(GRAPHS))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationGraphs('/api/graphs/s1/week', true))

    await waitFor(() => expect(result.current.data).toEqual(GRAPHS))
    expect(fetchMock).toHaveBeenCalledWith('/api/graphs/s1/week?exclude_new_stations=true')
  })

  it('does not fetch when the url is null', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationGraphs(null, true))

    expect(result.current.data).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
