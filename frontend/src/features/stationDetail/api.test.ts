import { ok } from '@/test-utils/http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  fetchGraphs,
  fetchMonthlyTotals,
  fetchOverviewStats,
  fetchStationDetailPage,
  withTrendParam,
} from './api'

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
  it('derives every card link locally and keeps the rest of the payload', async () => {
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
        overview: { href: '/api/bff/station-detail/s1/overview', templated: false },
        graphs_day: { href: '/api/bff/station-detail/s1/graphs/day', templated: false },
        graphs_week: {
          href: '/api/bff/station-detail/s1/graphs/week?as_of=2026-01-01T00:00:00Z',
          templated: false,
        },
        graphs_last_30_days: {
          href: '/api/bff/station-detail/s1/graphs/last_30_days',
          templated: false,
        },
        graphs_year: { href: '/api/bff/station-detail/s1/graphs/year', templated: false },
        monthly: { href: '/api/bff/station-detail/s1/monthly', templated: false },
      },
    }
    const fetchMock = stubFetch(ok(raw))

    const page = await fetchStationDetailPage('s1')

    expect(fetchMock).toHaveBeenCalledWith('/api/bff/station-detail/s1')
    expect(page.name).toBe('Zoo Station')
    // The links are rebuilt from the validated id; the response's own hrefs
    // (including the `as_of` query) are informational only and never used.
    expect(page._links.self).toBe('/api/bff/station-detail/s1')
    expect(page._links.overview).toBe('/api/bff/station-detail/s1/overview')
    expect(page._links.graphs_day).toBe('/api/bff/station-detail/s1/graphs/day')
    expect(page._links.graphs_week).toBe('/api/bff/station-detail/s1/graphs/week')
    expect(page._links.graphs_last_30_days).toBe('/api/bff/station-detail/s1/graphs/last_30_days')
    expect(page._links.graphs_year).toBe('/api/bff/station-detail/s1/graphs/year')
    expect(page._links.monthly).toBe('/api/bff/station-detail/s1/monthly')
  })

  it('rejects when the shell responds with a non-ok status', async () => {
    stubFetch(() => new Response('boom', { status: 500 }))

    await expect(fetchStationDetailPage('s1')).rejects.toThrow(
      '/api/bff/station-detail/s1 responded with 500',
    )
  })

  it('refuses a tainted station id without issuing a request (CSRF/SSRF guard)', async () => {
    const fetchMock = stubFetch(ok({}))

    // A route parameter taken straight from the browser URL must never be able
    // to change the requested route, nor to append its own query/fragment.
    for (const id of ['../api/v1/measurements', 's1/overview', 's1?url=https://evil', 's1#x']) {
      await expect(fetchStationDetailPage(id)).rejects.toThrow(
        'Refusing to use unsafe path segment',
      )
    }
    expect(fetchMock).not.toHaveBeenCalled()
  })
})

describe('withTrendParam', () => {
  it('returns the url unchanged when the flag is off', () => {
    expect(withTrendParam('/api/bff/station-detail/s1/graphs/day', false)).toBe(
      '/api/bff/station-detail/s1/graphs/day',
    )
  })

  it('appends the flag with a question mark when the url has no query', () => {
    expect(withTrendParam('/api/bff/station-detail/s1/graphs/day', true)).toBe(
      '/api/bff/station-detail/s1/graphs/day?exclude_new_stations=true',
    )
  })

  it('appends the flag with an ampersand when the url already has a query', () => {
    expect(
      withTrendParam('/api/bff/station-detail/s1/graphs/day?as_of=2026-01-01T00:00:00Z', true),
    ).toBe(
      '/api/bff/station-detail/s1/graphs/day?as_of=2026-01-01T00:00:00Z&exclude_new_stations=true',
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
    await expect(fetchOverviewStats('/api/bff/station-detail/s1/overview', false)).resolves.toEqual(
      STATS,
    )
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/station-detail/s1/overview')
  })

  it('forwards the exclude flag as a query param', async () => {
    const fetchMock = stubFetch(ok(STATS))
    await expect(fetchOverviewStats('/api/bff/station-detail/s1/overview', true)).resolves.toEqual(
      STATS,
    )
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/bff/station-detail/s1/overview?exclude_new_stations=true',
    )
  })

  it('rejects on a non-ok response', async () => {
    stubFetch(() => new Response('nope', { status: 404 }))
    await expect(fetchOverviewStats('/api/bff/station-detail/s1/overview', false)).rejects.toThrow(
      '/api/bff/station-detail/s1/overview responded with 404',
    )
  })
})

describe('fetchGraphs', () => {
  const GRAPHS = { current: [], previous: [] }

  it('fetches the url as-is when the flag is off', async () => {
    const fetchMock = stubFetch(ok(GRAPHS))
    await expect(fetchGraphs('/api/bff/station-detail/s1/graphs/week', false)).resolves.toEqual(
      GRAPHS,
    )
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/station-detail/s1/graphs/week')
  })

  it('appends the exclude flag when it is on', async () => {
    const fetchMock = stubFetch(ok(GRAPHS))
    await expect(
      fetchGraphs('/api/bff/station-detail/s1/graphs/week?as_of=x', true),
    ).resolves.toEqual(GRAPHS)
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/bff/station-detail/s1/graphs/week?as_of=x&exclude_new_stations=true',
    )
  })
})

describe('fetchMonthlyTotals', () => {
  const MONTHLY = { monthly_totals: [{ year: 2025, month: 1, total: 10 }] }

  it('returns the monthly totals payload on success', async () => {
    const fetchMock = stubFetch(ok(MONTHLY))
    await expect(fetchMonthlyTotals('/api/bff/station-detail/s1/monthly')).resolves.toEqual(MONTHLY)
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/station-detail/s1/monthly')
  })

  it('rejects on a non-ok response', async () => {
    stubFetch(() => new Response('error', { status: 503 }))
    await expect(fetchMonthlyTotals('/api/bff/station-detail/s1/monthly')).rejects.toThrow(
      '/api/bff/station-detail/s1/monthly responded with 503',
    )
  })
})
