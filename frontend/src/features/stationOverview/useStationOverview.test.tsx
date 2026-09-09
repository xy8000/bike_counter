import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useStationOverview } from './useStationOverview'
import type { StationOverviewStats } from './types'

const STATS: StationOverviewStats = {
  total_bikes: 123,
  metrics: [
    {
      key: 'last_day',
      current: 10,
      previous: 8,
      trend: 'up',
      delta_percent: 25,
      is_new: false,
    },
  ],
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function rawShell(id: string, statsHref: string) {
  return {
    id,
    name: id === 'zoo' ? 'Zoo Station' : 'Other Station',
    description: 'A station',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 2,
    image_url: '/img/a.png',
    last_update: null,
    detail_url: `/stations/${id}`,
    _links: { stats: { href: statsHref, templated: false } },
  }
}

interface ServerOptions {
  shellError?: boolean
  statsError?: boolean
}

function makeServer(options: ServerOptions = {}) {
  const state = { shellError: options.shellError ?? false, statsError: options.statsError ?? false }
  const statsHref = '/api/bff/station-overview/zoo/stats'
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (url.includes('/stats')) {
      if (state.statsError) return Promise.reject(new Error('stats failed'))
      return Promise.resolve(ok(STATS))
    }
    if (url.includes('/station-overview/zoo')) {
      if (state.shellError) return Promise.reject(new Error('shell failed'))
      return Promise.resolve(ok(rawShell('zoo', statsHref)))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return { mock, state, statsHref }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useStationOverview', () => {
  it('keeps everything cleared while the station id is null', () => {
    makeServer()
    const { result } = renderHook(() => useStationOverview(null))

    expect(result.current.page).toBeNull()
    expect(result.current.stats).toBeNull()
    expect(result.current.error).toBe(false)
    expect(result.current.statsError).toBe(false)
  })

  it('loads the shell and then the stats from the _links.stats href', async () => {
    const server = makeServer()
    const { result } = renderHook(() => useStationOverview('zoo'))

    expect(result.current.page).toBeNull()
    await waitFor(() => expect(result.current.page?.name).toBe('Zoo Station'))
    await waitFor(() => expect(result.current.stats).toEqual(STATS))

    expect(result.current.error).toBe(false)
    expect(result.current.statsError).toBe(false)
    expect(server.mock).toHaveBeenNthCalledWith(1, '/api/bff/station-overview/zoo')
    expect(server.mock).toHaveBeenNthCalledWith(2, server.statsHref)
  })

  it('clears the state again when the station id becomes null', async () => {
    makeServer()
    const { result, rerender } = renderHook(
      (props: { id: string | null }) => useStationOverview(props.id),
      { initialProps: { id: 'zoo' as string | null } },
    )
    await waitFor(() => expect(result.current.page).not.toBeNull())
    await waitFor(() => expect(result.current.stats).not.toBeNull())

    rerender({ id: null })

    expect(result.current.page).toBeNull()
    expect(result.current.stats).toBeNull()
    expect(result.current.error).toBe(false)
    expect(result.current.statsError).toBe(false)
  })

  it('sets the error when the shell request fails', async () => {
    makeServer({ shellError: true })
    const { result } = renderHook(() => useStationOverview('zoo'))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.page).toBeNull()
    expect(result.current.stats).toBeNull()
    expect(result.current.statsError).toBe(false)
  })

  it('keeps the shell and sets the stats error when only the stats request fails', async () => {
    makeServer({ statsError: true })
    const { result } = renderHook(() => useStationOverview('zoo'))

    await waitFor(() => expect(result.current.page).not.toBeNull())
    await waitFor(() => expect(result.current.statsError).toBe(true))
    expect(result.current.error).toBe(false)
    expect(result.current.stats).toBeNull()
  })

  it('ignores a stale shell response when the station id changes first', async () => {
    let resolveZoo!: (response: Response) => void
    const fetchMock = vi.fn((input: unknown) => {
      const url = String(input)
      if (url.includes('/stats')) return Promise.resolve(ok(STATS))
      if (url.includes('/station-overview/other')) {
        return Promise.resolve(ok(rawShell('other', '/api/bff/station-overview/other/stats')))
      }
      // The first (zoo) shell stays pending until the test resolves it.
      return new Promise<Response>((resolve) => {
        resolveZoo = resolve
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      (props: { id: string }) => useStationOverview(props.id),
      { initialProps: { id: 'zoo' } },
    )
    rerender({ id: 'other' })

    await waitFor(() => expect(result.current.page?.name).toBe('Other Station'))
    await waitFor(() => expect(result.current.stats).toEqual(STATS))

    // Resolving the stale zoo shell must not overwrite the other station.
    act(() => {
      resolveZoo(ok(rawShell('zoo', '/api/bff/station-overview/zoo/stats')))
    })
    await waitFor(() => expect(fetchMock.mock.calls.length).toBeGreaterThan(2))
    expect(result.current.page?.name).toBe('Other Station')
    expect(result.current.error).toBe(false)
    expect(result.current.statsError).toBe(false)
  })

  it('does not set stats when the component unmounts while the stats request is in flight', async () => {
    let resolveStats!: (response: Response) => void
    const fetchMock = vi.fn((input: unknown) => {
      const url = String(input)
      if (url.includes('/stats')) {
        return new Promise<Response>((resolve) => {
          resolveStats = resolve
        })
      }
      return Promise.resolve(ok(rawShell('zoo', '/api/bff/station-overview/zoo/stats')))
    })
    vi.stubGlobal('fetch', fetchMock)

    const { result, unmount } = renderHook(() => useStationOverview('zoo'))
    await waitFor(() => expect(result.current.page).not.toBeNull())

    unmount()
    act(() => {
      resolveStats(ok(STATS))
    })
    // No state update is attempted after unmount (no act warning / throw).
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })
})
