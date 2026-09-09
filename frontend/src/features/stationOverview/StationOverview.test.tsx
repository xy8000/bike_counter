import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MemoryRouter } from 'react-router-dom'
import { StationOverview } from './StationOverview'
import type { StationOverviewStats } from './types'

const SHELL_URL = '/api/bff/station-overview/zoo'
const STATS_URL = `${SHELL_URL}/stats`

const STATS: StationOverviewStats = {
  total_bikes: 1234,
  metrics: [
    {
      key: 'last_day',
      current: 12,
      previous: 10,
      trend: 'up',
      delta_percent: 20,
      is_new: false,
    },
    {
      key: 'last_7_days',
      current: 100,
      previous: 90,
      trend: 'down',
      delta_percent: -11,
      is_new: false,
    },
    {
      key: 'last_month',
      current: 300,
      previous: 250,
      trend: 'up',
      delta_percent: 20,
      is_new: false,
    },
    {
      key: 'last_year',
      current: 5000,
      previous: 3000,
      trend: 'flat',
      delta_percent: null,
      is_new: true,
    },
  ],
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function rawShell() {
  return {
    id: 'zoo',
    name: 'Zoo Station',
    description: 'A zoo by the river.',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 2,
    image_url: '/img/zoo.png',
    last_update: '2026-01-02T12:00:00Z',
    detail_url: '/stations/zoo',
    _links: { stats: { href: STATS_URL, templated: false } },
  }
}

interface ServerOptions {
  shellError?: boolean
  statsError?: boolean
}

function makeServer(options: ServerOptions = {}) {
  const state = { shellError: options.shellError ?? false, statsError: options.statsError ?? false }
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (url.includes('/stats')) {
      if (state.statsError) return Promise.reject(new Error('stats failed'))
      return Promise.resolve(ok(STATS))
    }
    if (url.includes('/station-overview/zoo')) {
      if (state.shellError) return Promise.reject(new Error('shell failed'))
      return Promise.resolve(ok(rawShell()))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return { mock, state }
}

function renderOverview() {
  const onClose = vi.fn()
  const utils = render(
    <MemoryRouter>
      <StationOverview stationId="zoo" onClose={onClose} />
    </MemoryRouter>,
  )
  return { ...utils, onClose }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('StationOverview', () => {
  it('shows the loading skeleton while the shell is pending', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise<Response>(() => {})),
    )
    const { container } = renderOverview()

    expect(screen.getByText('Counting station')).toBeInTheDocument()
    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull()
  })

  it('shows the shell error text when the overview request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')))
    renderOverview()

    expect(await screen.findByText('Could not load the station overview.')).toBeInTheDocument()
  })

  it('renders the loaded shell with the stats and the detail/close actions', async () => {
    const server = makeServer()
    const { onClose } = renderOverview()

    expect(await screen.findByRole('link', { name: 'Zoo Station' })).toHaveAttribute(
      'href',
      '/stations/zoo',
    )
    expect(screen.getByText('A zoo by the river.')).toBeInTheDocument()
    expect(screen.getByText('2 channels')).toBeInTheDocument()
    expect(screen.getByAltText('Zoo Station image')).toHaveAttribute('src', '/img/zoo.png')
    expect(screen.getByText(/^Updated /)).toBeInTheDocument()

    expect(await screen.findByText('Total bikes (all time)')).toBeInTheDocument()
    expect(screen.getByText('1.234')).toBeInTheDocument()
    expect(screen.getByText('Last 24 hours')).toBeInTheDocument()
    expect(screen.getByText('Last year')).toBeInTheDocument()

    expect(screen.getByRole('link', { name: 'Open detailed view' })).toHaveAttribute(
      'href',
      '/stations/zoo',
    )
    expect(screen.getByRole('link', { name: 'Open detail page' })).toHaveAttribute(
      'href',
      '/stations/zoo',
    )

    fireEvent.click(screen.getByRole('button', { name: 'Close station overview' }))
    expect(onClose).toHaveBeenCalledTimes(1)

    expect(server.mock).toHaveBeenCalledWith(SHELL_URL)
    expect(server.mock).toHaveBeenCalledWith(STATS_URL)
  })

  it('shows the stats error text when the stats sub-resource fails', async () => {
    makeServer({ statsError: true })
    renderOverview()

    expect(await screen.findByRole('link', { name: 'Zoo Station' })).toBeInTheDocument()
    expect(await screen.findByText('Could not load the overview stats.')).toBeInTheDocument()
  })

  it('shows the panel skeleton while the shell is present but the stats are pending', async () => {
    let resolveStats!: (response: Response) => void
    const fetchMock = vi.fn((input: unknown) => {
      const url = String(input)
      if (url.includes('/stats')) {
        return new Promise<Response>((resolve) => {
          resolveStats = resolve
        })
      }
      return Promise.resolve(ok(rawShell()))
    })
    vi.stubGlobal('fetch', fetchMock)

    const { container } = renderOverview()
    await screen.findByRole('link', { name: 'Zoo Station' })

    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull()

    act(() => {
      resolveStats(ok(STATS))
    })
    expect(await screen.findByText('Total bikes (all time)')).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })
})
