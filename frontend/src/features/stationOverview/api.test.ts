import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchStationOverview, fetchStationOverviewStats } from './api'

const SHELL_URL = '/api/bff/station-overview/zoo'
const STATS_URL = '/api/bff/station-overview/zoo/stats'

function ok(data: unknown, status = 200) {
  return new Response(JSON.stringify(data), { status })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('fetchStationOverview', () => {
  const rawShell = {
    id: 'zoo',
    name: 'Zoo Station',
    description: 'Near the zoo',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 2,
    image_url: '/img/zoo.png',
    last_update: null,
    detail_url: '/stations/zoo',
    _links: { stats: { href: STATS_URL, templated: false } },
  }

  it('fetches the shell and unwraps the _links.stats LinkDto to its href', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(rawShell))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchStationOverview('zoo')).resolves.toEqual({
      ...rawShell,
      _links: { stats: STATS_URL },
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith(SHELL_URL)
  })

  it('rejects when the shell request is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({}, 404)))

    await expect(fetchStationOverview('zoo')).rejects.toThrow(`${SHELL_URL} responded with 404`)
  })
})

describe('fetchStationOverviewStats', () => {
  const stats = { total_bikes: 5, metrics: [] }

  it('fetches the stats card through the given href', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(stats))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchStationOverviewStats(STATS_URL)).resolves.toEqual(stats)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith(STATS_URL)
  })

  it('rejects when the stats request is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({}, 500)))

    await expect(fetchStationOverviewStats(STATS_URL)).rejects.toThrow(
      `${STATS_URL} responded with 500`,
    )
  })
})
