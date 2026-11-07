import { ok } from '@/test-utils/http'
import { SUMMARY_BOUNDS, rawSummaryPage } from '@/test-utils/stationsSummary'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  fetchStationsSummaryPage,
  fetchSummaryGraphs,
  fetchSummaryMonthly,
  fetchSummaryOverview,
} from './api'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('stationsSummary api', () => {
  describe('fetchStationsSummaryPage', () => {
    it('builds the bounds-only query and unwraps the _links hrefs', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok(rawSummaryPage()))
      vi.stubGlobal('fetch', fetchMock)

      const page = await fetchStationsSummaryPage(SUMMARY_BOUNDS)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/bff/stations/summary?min_lat=51&min_lng=7&max_lat=52&max_lng=8',
      )
      expect(page.image_url).toBe('/img/summary.png')
      // Each `_links` value is unwrapped from a `LinkDto` to its bare `href`.
      expect(page._links.self).toBe('/api/bff/stations/summary')
      expect(page._links.overview).toBe('/api/bff/stations/summary/overview')
      expect(page._links.graphs_week).toBe('/api/bff/stations/summary/graphs/week')
      expect(page._links.monthly).toBe('/api/bff/stations/summary/monthly')
    })

    it('rejects when the shell responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(fetchStationsSummaryPage(SUMMARY_BOUNDS)).rejects.toThrow(
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

      await fetchSummaryOverview('/api/bff/stations/summary/overview', [], false)

      expect(fetchMock).toHaveBeenCalledWith('/api/bff/stations/summary/overview')
    })

    it('appends the exclude ids and the new-stations flag to a bare link', async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValue(ok({ channel_count: 4, total_bikes: 10, metrics: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryOverview('/api/bff/stations/summary/overview', ['s1', 's2'], true)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/bff/stations/summary/overview?exclude=s1,s2&exclude_new_stations=true',
      )
    })

    it('appends the params to a link that already carries a query', async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValue(ok({ channel_count: 4, total_bikes: 10, metrics: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryOverview(
        '/api/bff/stations/summary/overview?as_of=2026-01-01',
        ['s1'],
        true,
      )

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/bff/stations/summary/overview?as_of=2026-01-01&exclude=s1&exclude_new_stations=true',
      )
    })

    it('rejects when the overview responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 404 })))

      await expect(
        fetchSummaryOverview('/api/bff/stations/summary/overview', ['s1'], false),
      ).rejects.toThrow('/api/bff/stations/summary/overview?exclude=s1 responded with 404')
    })

    it('refuses an exclude id that would inject another parameter (CSRF/SSRF guard)', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({}))
      vi.stubGlobal('fetch', fetchMock)

      await expect(
        fetchSummaryOverview(
          '/api/bff/stations/summary/overview',
          ['s1&url=https://evil.example'],
          false,
        ),
      ).rejects.toThrow('Refusing to use unsafe path segment')
      expect(fetchMock).not.toHaveBeenCalled()
    })
  })

  describe('fetchSummaryGraphs', () => {
    it('fetches the graphs link with the derived query params', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({}))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryGraphs(
        '/api/bff/stations/summary/graphs/week?resolution=hour',
        ['s1'],
        true,
      )

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/bff/stations/summary/graphs/week?resolution=hour&exclude=s1&exclude_new_stations=true',
      )
    })

    it('fetches the link unchanged when nothing is excluded', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({}))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryGraphs('/api/bff/stations/summary/graphs/year', [], false)

      expect(fetchMock).toHaveBeenCalledWith('/api/bff/stations/summary/graphs/year')
    })

    it('rejects when the graphs card responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(
        fetchSummaryGraphs('/api/bff/stations/summary/graphs/week', ['s1'], false),
      ).rejects.toThrow('/api/bff/stations/summary/graphs/week?exclude=s1 responded with 500')
    })
  })

  describe('fetchSummaryMonthly', () => {
    it('fetches the monthly link with the derived query params', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({ monthly_totals: [] }))
      vi.stubGlobal('fetch', fetchMock)

      await fetchSummaryMonthly('/api/bff/stations/summary/monthly?as_of=2026-01-01', [], true)

      expect(fetchMock).toHaveBeenCalledWith(
        '/api/bff/stations/summary/monthly?as_of=2026-01-01&exclude_new_stations=true',
      )
    })

    it('rejects when the monthly card responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(
        fetchSummaryMonthly('/api/bff/stations/summary/monthly', [], false),
      ).rejects.toThrow('/api/bff/stations/summary/monthly responded with 500')
    })
  })
})
