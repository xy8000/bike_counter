import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Bounds } from '../../lib/geo'
import {
  fetchStationsSummaryPage,
  fetchSummaryGraphs,
  fetchSummaryMonthly,
  fetchSummaryOverview,
} from './api'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const BOUNDS: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }

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

describe('stationsSummary api', () => {
  describe('fetchStationsSummaryPage', () => {
    it('builds the bounds-only query and unwraps the _links hrefs', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok(rawPage()))
      vi.stubGlobal('fetch', fetchMock)

      const page = await fetchStationsSummaryPage(BOUNDS)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/bff/stations/summary?min_lat=51&min_lng=7&max_lat=52&max_lng=8',
      )
      expect(page.image_url).toBe('/img/summary.png')
      // Each `_links` value is unwrapped from a `LinkDto` to its bare `href`.
      expect(page._links.self).toBe('/api/summary/self')
      expect(page._links.overview).toBe('/api/overview/summary')
      expect(page._links.graphs_week).toBe('/api/graphs/summary/week')
      expect(page._links.monthly).toBe('/api/monthly/summary')
    })

    it('rejects when the shell responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(fetchStationsSummaryPage(BOUNDS)).rejects.toThrow(
        '/api/bff/stations/summary?min_lat=51&min_lng=7&max_lat=52&max_lng=8 responded with 500',
      )
    })
  })

  describe('fetchSummaryOverview', () => {
    it('fetches the link unchanged with no exclude set or Bike-Trends flag', async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValue(ok({ channel_count: 4, total_bikes: 10, metrics: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryOverview('/api/overview/summary', [], false)

      expect(fetchMock).toHaveBeenCalledWith('/api/overview/summary')
    })

    it('appends the exclude ids and the new-stations flag to a bare link', async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValue(ok({ channel_count: 4, total_bikes: 10, metrics: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryOverview('/api/overview/summary', ['s1', 's2'], true)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/overview/summary?exclude=s1,s2&exclude_new_stations=true',
      )
    })

    it('appends the params to a link that already carries a query', async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValue(ok({ channel_count: 4, total_bikes: 10, metrics: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryOverview('/api/overview/summary?as_of=2026-01-01', ['s1'], true)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/overview/summary?as_of=2026-01-01&exclude=s1&exclude_new_stations=true',
      )
    })

    it('rejects when the overview responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 404 })))

      await expect(fetchSummaryOverview('/api/overview/summary', ['s1'], false)).rejects.toThrow(
        '/api/overview/summary?exclude=s1 responded with 404',
      )
    })
  })

  describe('fetchSummaryGraphs', () => {
    it('fetches the graphs link with the derived query params', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({}))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryGraphs('/api/graphs/summary/week?resolution=hour', ['s1'], true)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/graphs/summary/week?resolution=hour&exclude=s1&exclude_new_stations=true',
      )
    })

    it('fetches the link unchanged when nothing is excluded', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({}))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryGraphs('/api/graphs/summary/year', [], false)

      expect(fetchMock).toHaveBeenCalledWith('/api/graphs/summary/year')
    })

    it('rejects when the graphs card responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(fetchSummaryGraphs('/api/graphs/summary/week', ['s1'], false)).rejects.toThrow(
        '/api/graphs/summary/week?exclude=s1 responded with 500',
      )
    })
  })

  describe('fetchSummaryMonthly', () => {
    it('fetches the monthly link with the derived query params', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({ monthly_totals: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryMonthly('/api/monthly/summary?as_of=2026-01-01', [], true)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/monthly/summary?as_of=2026-01-01&exclude_new_stations=true',
      )
    })

    it('rejects when the monthly card responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(fetchSummaryMonthly('/api/monthly/summary', [], false)).rejects.toThrow(
        '/api/monthly/summary responded with 500',
      )
    })
  })
})
