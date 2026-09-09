import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchDataSourceDetail, fetchDataSources } from './api'
import type { DataSourceDetail, DataSourceSummary } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const SUMMARY: DataSourceSummary = {
  id: 'ms',
  name: 'Münster',
  provider_type: 'radvis',
  last_updated_at: '2026-01-01T10:00:00Z',
  station_count: 2,
  channel_count: 4,
  image_url: '/logos/ms.png',
  last_import: null,
}

const DETAIL: DataSourceDetail = {
  id: 'ms',
  name: 'Münster',
  provider_type: 'radvis',
  image_url: '/logos/ms.png',
  station_count: 2,
  channel_count: 4,
  stations: [],
  last_updated_at: '2026-01-01T10:00:00Z',
  imported_until: null,
  first_data_at: null,
  last_data_at: null,
  has_historical: true,
  has_real_time: true,
  has_full_current_year: false,
  last_import: null,
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('dataSources api', () => {
  describe('fetchDataSources', () => {
    it('fetches the overview and returns the wrapped items', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok({ items: [SUMMARY] }))
      vi.stubGlobal('fetch', fetchMock)

      const items = await fetchDataSources()

      expect(fetchMock).toHaveBeenCalledWith('/api/bff/data-sources')
      expect(items).toEqual([SUMMARY])
    })

    it('rejects when the overview responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

      await expect(fetchDataSources()).rejects.toThrow('/api/bff/data-sources responded with 500')
    })
  })

  describe('fetchDataSourceDetail', () => {
    it('fetches the detail payload for the data-source id', async () => {
      const fetchMock = vi.fn().mockResolvedValue(ok(DETAIL))
      vi.stubGlobal('fetch', fetchMock)

      const detail = await fetchDataSourceDetail('ms')

      expect(fetchMock).toHaveBeenCalledWith('/api/bff/data-sources/ms')
      expect(detail).toEqual(DETAIL)
    })

    it('rejects when the detail responds with a non-ok status', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 404 })))

      await expect(fetchDataSourceDetail('ms')).rejects.toThrow(
        '/api/bff/data-sources/ms responded with 404',
      )
    })
  })
})
