import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationDetailPage } from './useStationDetailPage'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function shell(id: string) {
  return {
    id,
    name: `Station ${id}`,
    description: 'A station',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 1,
    image_url: '/img/s.png',
    last_update: null,
    channels: [],
    _links: {
      self: { href: `/stations/${id}`, templated: false },
      overview: { href: `/api/overview/${id}`, templated: false },
      graphs_day: { href: `/api/graphs/${id}/day`, templated: false },
      graphs_week: { href: `/api/graphs/${id}/week`, templated: false },
      graphs_last_30_days: { href: `/api/graphs/${id}/last_30_days`, templated: false },
      graphs_year: { href: `/api/graphs/${id}/year`, templated: false },
      monthly: { href: `/api/monthly/${id}`, templated: false },
    },
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationDetailPage', () => {
  it('loads and unwraps the detail shell for a station id', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(shell('s1')))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useStationDetailPage('s1'))

    expect(result.current.loading).toBe(true)
    expect(result.current.data).toBeNull()

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe(false)
    expect(result.current.data?.name).toBe('Station s1')
    expect(result.current.data?._links.overview).toBe('/api/overview/s1')
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/station-detail/s1')
  })

  it('reports the error when the shell request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))

    const { result } = renderHook(() => useStationDetailPage('s1'))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.data).toBeNull()
  })

  it('refetches when the station id changes', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(shell('s1')))
      .mockResolvedValueOnce(ok(shell('s2')))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ id }: { id: string | null }) => useStationDetailPage(id),
      { initialProps: { id: 's1' } },
    )
    await waitFor(() => expect(result.current.data?.name).toBe('Station s1'))

    rerender({ id: 's2' })
    expect(result.current.data).toBeNull()
    expect(result.current.loading).toBe(true)

    await waitFor(() => expect(result.current.data?.name).toBe('Station s2'))
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('resets and does not fetch when the station id is null', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(shell('s1')))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook<
      ReturnType<typeof useStationDetailPage>,
      { id: string | null }
    >(({ id }) => useStationDetailPage(id), { initialProps: { id: 's1' } })
    await waitFor(() => expect(result.current.data?.name).toBe('Station s1'))

    rerender({ id: null })
    expect(result.current.data).toBeNull()
    expect(result.current.error).toBe(false)
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})
