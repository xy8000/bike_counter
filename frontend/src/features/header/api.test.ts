import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchGlobalSummary } from './api'
import type { GlobalSummary } from './types'

const SUMMARY: GlobalSummary = {
  station_count: 12,
  channel_count: 24,
  bikes_last_day_total: 1000,
  last_update: '2026-01-02T12:00:00Z',
}

function ok(data: unknown, status = 200) {
  return new Response(JSON.stringify(data), { status })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('fetchGlobalSummary', () => {
  it('fetches the plain summary when new stations are included', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(SUMMARY))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchGlobalSummary(false)).resolves.toEqual(SUMMARY)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/global-summary')
  })

  it('appends exclude_new_stations=true when new stations are excluded', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(SUMMARY))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchGlobalSummary(true)).resolves.toEqual(SUMMARY)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/global-summary?exclude_new_stations=true')
  })

  it('rejects when the response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({}, 500)))

    await expect(fetchGlobalSummary(false)).rejects.toThrow('global summary responded with 500')
  })
})
