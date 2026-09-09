import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import { StationDetail } from './StationDetail'
import type { MonthTotal, PeriodGraphs, StationOverviewStats } from './types'

// The shared recharts mock eagerly JSON.stringify's every non-children prop.
// The shadcn ChartTooltip/ChartLegend aliases forward a `content` React element
// (whose fiber owner is circular), which would crash that serialization. Keep
// the real ChartContainer/ChartTooltipContent but null the two sinks.
vi.mock('@/components/ui/chart', async () => {
  const actual =
    await vi.importActual<typeof import('@/components/ui/chart')>('@/components/ui/chart')
  return { ...actual, ChartTooltip: () => null, ChartLegend: () => null }
})

const SHELL_URL = '/api/bff/station-detail/s1'
const OVERVIEW_URL = '/api/overview/s1'
const GRAPHS_WEEK_URL = '/api/graphs/s1/week'
const GRAPHS_YEAR_URL = '/api/graphs/s1/year'
const MONTHLY_URL = '/api/monthly/s1'

const GLOBAL_SUMMARY = {
  station_count: 4,
  channel_count: 8,
  bikes_last_day_total: 120,
  last_update: null,
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function rawShell() {
  return {
    id: 's1',
    name: 'Zoo Station',
    description: 'A zoo by the river.',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 2,
    image_url: '/img/zoo.png',
    last_update: '2026-01-02T12:00:00Z',
    channels: [
      { id: 'c1', name: 'North' },
      { id: 'c2', name: 'South' },
    ],
    _links: {
      self: { href: '/stations/s1', templated: false },
      overview: { href: OVERVIEW_URL, templated: false },
      graphs_day: { href: '/api/graphs/s1/day', templated: false },
      graphs_week: { href: GRAPHS_WEEK_URL, templated: false },
      graphs_last_30_days: { href: '/api/graphs/s1/last_30_days', templated: false },
      graphs_year: { href: GRAPHS_YEAR_URL, templated: false },
      monthly: { href: MONTHLY_URL, templated: false },
    },
  }
}

const OVERVIEW: StationOverviewStats = {
  total_bikes: 1234,
  metrics: [
    { key: 'last_day', current: 12, previous: 10, trend: 'up', delta_percent: 20, is_new: false },
    {
      key: 'last_7_days',
      current: 100,
      previous: 90,
      trend: 'down',
      delta_percent: -11,
      is_new: false,
    },
  ],
}

function graphs(): PeriodGraphs {
  return {
    current: [
      { start: '2024-01-01T12:00:00.000Z', total: 5 },
      { start: '2024-01-01T13:00:00.000Z', total: 7 },
    ],
    previous: [],
    weekday_radar: [{ weekday: 3, total: 9 }],
    weekday_radar_previous: [],
    hourly: [{ hour: 13, total: 7 }],
    hourly_previous: [],
    channel_pie: [
      { channel_id: 'c1', total: 8 },
      { channel_id: 'c2', total: 4 },
    ],
    per_channel: [
      {
        channel_id: 'c1',
        current: [{ start: '2024-01-01T12:00:00.000Z', total: 8 }],
        previous: [],
        weekday_radar: [{ weekday: 3, total: 8 }],
        weekday_radar_previous: [],
        hourly: [{ hour: 13, total: 8 }],
        hourly_previous: [],
      },
    ],
    is_new: false,
  }
}

const MONTHLY: MonthTotal[] = [
  { year: 2024, month: 1, total: 12 },
  { year: 2025, month: 1, total: 20 },
]

interface ServerOptions {
  shellError?: boolean
  overviewError?: boolean
  graphsError?: boolean
  monthlyError?: boolean
  /** When set, the graphs card reports the station opened during the period. */
  graphsNew?: boolean
}

function makeServer(options: ServerOptions = {}) {
  const state = {
    shellError: options.shellError ?? false,
    overviewError: options.overviewError ?? false,
    graphsError: options.graphsError ?? false,
    monthlyError: options.monthlyError ?? false,
    graphsNew: options.graphsNew ?? false,
  }
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (url.includes('/api/bff/station-detail/')) {
      if (state.shellError) return Promise.reject(new Error('shell failed'))
      return Promise.resolve(ok(rawShell()))
    }
    // The BaseMap tiles-readiness probe: the archive is served so the map
    // preview mounts under jsdom.
    if (url.includes('/tiles/map.pmtiles')) {
      return Promise.resolve(new Response(null, { status: 200 }))
    }
    if (url.includes('/styles/basemap')) {
      return Promise.resolve(ok({ sources: {} }))
    }
    if (url.includes('/global-summary')) {
      return Promise.resolve(ok(GLOBAL_SUMMARY))
    }
    if (url.includes('/overview/')) {
      if (state.overviewError) return Promise.reject(new Error('overview failed'))
      return Promise.resolve(ok(OVERVIEW))
    }
    if (url.includes('/graphs/')) {
      if (state.graphsError) return Promise.reject(new Error('graphs failed'))
      const payload = graphs()
      return Promise.resolve(ok(state.graphsNew ? { ...payload, is_new: true } : payload))
    }
    if (url.includes('/monthly')) {
      if (state.monthlyError) return Promise.reject(new Error('monthly failed'))
      return Promise.resolve(ok({ monthly_totals: MONTHLY }))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return { mock, state }
}

function LocationProbe() {
  const location = useLocation()
  return (
    <div data-testid="location">
      {location.pathname}
      {location.search}
    </div>
  )
}

type Entry = string | { pathname: string; search: string; key: string }

function renderPage(initialEntries: Entry[]) {
  return render(
    <MemoryRouter initialEntries={initialEntries}>
      <TrendSettingsProvider>
        <Routes>
          <Route path="/stations/:stationId" element={<StationDetail />} />
          {/* Match the map routes the detail page navigates back to. */}
          <Route path="*" element={null} />
        </Routes>
        <LocationProbe />
      </TrendSettingsProvider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  localStorage.clear()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('StationDetail', () => {
  it('shows the shell loading skeleton while the shell request is pending', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise<Response>(() => {})),
    )
    const { container } = renderPage(['/stations/s1?timeframe=week'])

    expect(screen.getByRole('link', { name: 'Back to map' })).toBeInTheDocument()
    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull()
  })

  it('shows the shell error text when the detail request fails', async () => {
    makeServer({ shellError: true })
    renderPage(['/stations/s1?timeframe=week'])

    expect(await screen.findByText('Could not load the station.')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Zoo Station' })).not.toBeInTheDocument()
  })

  it('renders the loaded shell with its metadata, map, cards and key facts', async () => {
    const server = makeServer()
    renderPage(['/stations/s1?timeframe=week'])

    // Shell metadata.
    expect(await screen.findByRole('heading', { name: 'Zoo Station' })).toBeInTheDocument()
    expect(screen.getByText('A zoo by the river.')).toBeInTheDocument()
    expect(screen.getByText('2 channels')).toBeInTheDocument()
    expect(screen.getByAltText('Zoo Station image')).toHaveAttribute('src', '/img/zoo.png')
    expect(screen.getByText(/^Updated /)).toBeInTheDocument()

    // Map preview marker (mounts once the basemap style sub-request resolves).
    expect(await screen.findByAltText('Zoo Station')).toBeInTheDocument()

    // Overview card.
    expect(await screen.findByText('Total bikes (all time)')).toBeInTheDocument()
    expect(screen.getByText('1.234')).toBeInTheDocument()
    expect(screen.getByText('Last 24 hours')).toBeInTheDocument()
    expect(screen.getByText('Last 7 days')).toBeInTheDocument()

    // Detailed statistics + key facts derived from the graphs card.
    expect(screen.getByRole('heading', { name: 'Detailed statistics' })).toBeInTheDocument()
    expect(screen.getByText('Total bikes in selection')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Detailed stats' })).toBeInTheDocument()
    // Channel pie legend from the graphs channel totals.
    expect(screen.getByText('North · 8')).toBeInTheDocument()
    expect(screen.getByText('South · 4')).toBeInTheDocument()

    // Yearly bar chart from the monthly totals.
    expect(screen.getByRole('heading', { name: 'Bikes per month' })).toBeInTheDocument()
    expect(
      screen.getAllByRole('button').some((button) => button.textContent?.includes('2025')),
    ).toBe(true)

    // The windowed cards were fetched through the shell's HATEOAS links with the
    // derived resolution token and no Bike-Trends flag.
    expect(server.mock).toHaveBeenCalledWith(SHELL_URL)
    expect(server.mock).toHaveBeenCalledWith(OVERVIEW_URL)
    expect(server.mock).toHaveBeenCalledWith(`${GRAPHS_WEEK_URL}?resolution=hour`)
    expect(server.mock).toHaveBeenCalledWith(MONTHLY_URL)
  })

  it('shows the stats error text when the graphs sub-resource fails', async () => {
    makeServer({ graphsError: true })
    renderPage(['/stations/s1?timeframe=week'])

    expect(await screen.findByText('Could not load the statistics.')).toBeInTheDocument()
    expect(screen.getByText('Could not load the detailed stats.')).toBeInTheDocument()
    // The monthly card still loads independently.
    expect(screen.getByText('Bikes per month')).toBeInTheDocument()
  })

  it('shows the overview error text when the overview sub-resource fails', async () => {
    makeServer({ overviewError: true })
    renderPage(['/stations/s1?timeframe=week'])

    expect(await screen.findByText('Could not load the overview.')).toBeInTheDocument()
  })

  it('shows the monthly error text when the monthly sub-resource fails', async () => {
    makeServer({ monthlyError: true })
    renderPage(['/stations/s1?timeframe=week'])

    expect(await screen.findByText('Could not load the monthly totals.')).toBeInTheDocument()
  })

  it('opens the settings dialog and switching the timeframe refetches the graphs link', async () => {
    const server = makeServer()
    renderPage(['/stations/s1?timeframe=week'])

    await screen.findByText('Total bikes (all time)')

    fireEvent.click(screen.getByRole('button', { name: 'Calculation settings' }))
    expect(await screen.findByText('Bike-Trends settings')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'This year' }))

    // The year timeframe at the default mid resolution uses the year link + the
    // `week` granularity.
    await waitFor(() =>
      expect(server.mock).toHaveBeenCalledWith(`${GRAPHS_YEAR_URL}?resolution=week`),
    )
  })

  it('appends the Bike-Trends flag when the shared link carries exclude_new_stations', async () => {
    const server = makeServer()
    renderPage(['/stations/s1?timeframe=week&exclude_new_stations=1'])

    await waitFor(() =>
      expect(server.mock).toHaveBeenCalledWith(`${OVERVIEW_URL}?exclude_new_stations=true`),
    )
    await waitFor(() =>
      expect(server.mock).toHaveBeenCalledWith(
        `${GRAPHS_WEEK_URL}?resolution=hour&exclude_new_stations=true`,
      ),
    )
  })

  it('shows the new-station notice when compare is on and the station is new', async () => {
    makeServer({ graphsNew: true })
    renderPage(['/stations/s1?timeframe=week&compare=1'])

    expect(
      await screen.findByText(
        'This station opened during the compared period — the previous period comparison is not shown.',
      ),
    ).toBeInTheDocument()
  })

  it('navigates to the plain map route via the Back to map link on a deep link', async () => {
    makeServer()
    // A deep/shared link has the initial location key "default".
    renderPage([{ pathname: '/stations/s1', search: '?timeframe=week', key: 'default' }])

    const back = await screen.findByRole('link', { name: 'Back to map' })
    fireEvent.click(back)

    await waitFor(() => expect(screen.getByTestId('location').textContent).toBe('/'))
  })

  it('goes back in history from an in-app navigation (Back to map)', async () => {
    makeServer()
    renderPage(['/map?x=1', '/stations/s1?timeframe=week'])

    const back = await screen.findByRole('link', { name: 'Back to map' })
    fireEvent.click(back)

    await waitFor(() => expect(screen.getByTestId('location').textContent).toBe('/map?x=1'))
  })
})
