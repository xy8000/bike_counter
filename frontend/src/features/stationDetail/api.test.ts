import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  fetchGraphs,
  fetchMonthlyTotals,
  fetchOverviewStats,
  fetchStationDetailPage,
  withTrendParam,
} from './api'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function stubFetch(response: Response | (() => Response)) {
  const fetchMock = vi.fn(() =>
    Promise.resolve(typeof response === 'function' ? response() : response),
  )
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('fetchStationDetailPage', () => {
  it('unwraps every _links LinkDto href and keeps the rest of the payload', async () => {
    const raw = {
      id: 's1',
      name: 'Zoo Station',
      description: 'By the river',
      latitude: 51.96,
      longitude: 7.63,
      channel_count: 2,
      image_url: '/img/zoo.png',
      last_update: null,
      channels: [{ id: 'c1', name: 'North' }],
      _links: {
        self: { href: '/stations/s1', templated: false },
        overview: { href: '/api/overview/s1', templated: false },
        graphs_day: { href: '/api/graphs/s1/day', templated: false },
        graphs_week: { href: '/api/graphs/s1/week?as_of=2026-01-01T00:00:00Z', templated: false },
        graphs_last_30_days: { href: '/api/graphs/s1/last_30_days', templated: false },
        graphs_year: { href: '/api/graphs/s1/year', templated: false },
        monthly: { href: '/api/monthly/s1', templated: false },
      },
    }
    const fetchMock = stubFetch(ok(raw))

    const page = await fetchStationDetailPage('s1')

    expect(fetchMock).toHaveBeenCalledWith('/api/bff/station-detail/s1')
    expect(page.name).toBe('Zoo Station')
    expect(page._links.self).toBe('/stations/s1')
    expect(page._links.overview).toBe('/api/overview/s1')
    expect(page._links.graphs_day).toBe('/api/graphs/s1/day')
    expect(page._links.graphs_week).toBe('/api/graphs/s1/week?as_of=2026-01-01T00:00:00Z')
    expect(page._links.graphs_last_30_days).toBe('/api/graphs/s1/last_30_days')
    expect(page._links.graphs_year).toBe('/api/graphs/s1/year')
    expect(page._links.monthly).toBe('/api/monthly/s1')
  })

  it('rejects when the shell responds with a non-ok status', async () => {
    stubFetch(() => new Response('boom', { status: 500 }))

    await expect(fetchStationDetailPage('s1')).rejects.toThrow(
      '/api/bff/station-detail/s1 responded with 500',
    )
  })
})

describe('withTrendParam', () => {
  it('returns the url unchanged when the flag is off', () => {
    expect(withTrendParam('/api/graphs/s1/day', false)).toBe('/api/graphs/s1/day')
  })

  it('appends the flag with a question mark when the url has no query', () => {
    expect(withTrendParam('/api/graphs/s1/day', true)).toBe(
      '/api/graphs/s1/day?exclude_new_stations=true',
    )
  })

  it('appends the flag with an ampersand when the url already has a query', () => {
    expect(withTrendParam('/api/graphs/s1/day?as_of=2026-01-01T00:00:00Z', true)).toBe(
      '/api/graphs/s1/day?as_of=2026-01-01T00:00:00Z&exclude_new_stations=true',
    )
  })
})

describe('fetchOverviewStats', () => {
  const STATS = {
    total_bikes: 42,
    metrics: [],
  }

  it('fetches the url as-is when the flag is off', async () => {
    const fetchMock = stubFetch(ok(STATS))
    await expect(fetchOverviewStats('/api/overview/s1', false)).resolves.toEqual(STATS)
    expect(fetchMock).toHaveBeenCalledWith('/api/overview/s1')
  })

  it('forwards the exclude flag as a query param', async () => {
    const fetchMock = stubFetch(ok(STATS))
    await expect(fetchOverviewStats('/api/overview/s1', true)).resolves.toEqual(STATS)
    expect(fetchMock).toHaveBeenCalledWith('/api/overview/s1?exclude_new_stations=true')
  })

  it('rejects on a non-ok response', async () => {
    stubFetch(() => new Response('nope', { status: 404 }))
    await expect(fetchOverviewStats('/api/overview/s1', false)).rejects.toThrow(
      '/api/overview/s1 responded with 404',
    )
  })
})

describe('fetchGraphs', () => {
  const GRAPHS = { current: [], previous: [] }

  it('fetches the url as-is when the flag is off', async () => {
    const fetchMock = stubFetch(ok(GRAPHS))
    await expect(fetchGraphs('/api/graphs/s1/week', false)).resolves.toEqual(GRAPHS)
    expect(fetchMock).toHaveBeenCalledWith('/api/graphs/s1/week')
  })

  it('appends the exclude flag when it is on', async () => {
    const fetchMock = stubFetch(ok(GRAPHS))
    await expect(fetchGraphs('/api/graphs/s1/week?as_of=x', true)).resolves.toEqual(GRAPHS)
    expect(fetchMock).toHaveBeenCalledWith('/api/graphs/s1/week?as_of=x&exclude_new_stations=true')
  })
})

describe('fetchMonthlyTotals', () => {
  const MONTHLY = { monthly_totals: [{ year: 2025, month: 1, total: 10 }] }

  it('returns the monthly totals payload on success', async () => {
    const fetchMock = stubFetch(ok(MONTHLY))
    await expect(fetchMonthlyTotals('/api/monthly/s1')).resolves.toEqual(MONTHLY)
    expect(fetchMock).toHaveBeenCalledWith('/api/monthly/s1')
  })

  it('rejects on a non-ok response', async () => {
    stubFetch(() => new Response('error', { status: 503 }))
    await expect(fetchMonthlyTotals('/api/monthly/s1')).rejects.toThrow(
      '/api/monthly/s1 responded with 503',
    )
  })
})
