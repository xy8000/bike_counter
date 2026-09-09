import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import { StationsSummary } from './StationsSummary'
import type { SummaryStation } from './types'

// The shared recharts mock eagerly JSON.stringify's every non-children prop.
// The shadcn ChartTooltip/ChartLegend aliases forward a `content` React element
// (whose fiber owner is circular), which would crash that serialization. Keep
// the real ChartContainer/ChartTooltipContent but null the two sinks.
vi.mock('@/components/ui/chart', async () => {
  const actual =
    await vi.importActual<typeof import('@/components/ui/chart')>('@/components/ui/chart')
  return { ...actual, ChartTooltip: () => null, ChartLegend: () => null }
})

const BASE_QUERY = 'min_lat=51&min_lng=7&max_lat=52&max_lng=8'
const ENTRY = `/summary?${BASE_QUERY}&timeframe=week`

const GLOBAL_SUMMARY = {
  station_count: 2,
  channel_count: 5,
  bikes_last_day_total: 120,
  last_update: null,
}

const DEFAULT_STATIONS: SummaryStation[] = [
  { id: 's1', name: 'A-Stadt', latitude: 51.95, longitude: 7.62, channel_count: 2 },
  { id: 's2', name: 'B-Stadt', latitude: 51.97, longitude: 7.64, channel_count: 3 },
]

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function stationsForCount(count: number): SummaryStation[] {
  const extra = Array.from(
    { length: Math.max(0, count - DEFAULT_STATIONS.length) },
    (_, index) => ({
      id: `sx${index + 1}`,
      name: `Station ${index + 1}`,
      latitude: 51.9 + index / 100,
      longitude: 7.6,
      channel_count: 1,
    }),
  )
  return [...DEFAULT_STATIONS, ...extra].slice(0, count)
}

function rawShell(stations: SummaryStation[]) {
  return {
    image_url: '/img/summary.png',
    stations,
    last_update: '2026-01-02T12:00:00Z',
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

const OVERVIEW = {
  channel_count: 5,
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

const bucket = (total: number) => ({ start: '2024-01-01T12:00:00.000Z', total })

function graphsFor(stations: SummaryStation[], empty: boolean) {
  if (empty) {
    return {
      current: [],
      previous: [],
      weekday_radar: [],
      weekday_radar_previous: [],
      hourly: [],
      hourly_previous: [],
      station_pie: [],
      per_station: [],
    }
  }
  return {
    current: [
      { start: '2024-01-01T12:00:00.000Z', total: 5 },
      { start: '2024-01-01T13:00:00.000Z', total: 7 },
    ],
    previous: [{ start: '2024-01-01T12:00:00.000Z', total: 2 }],
    weekday_radar: [
      { weekday: 3, total: 9 },
      { weekday: 5, total: 3 },
    ],
    weekday_radar_previous: [{ weekday: 3, total: 4 }],
    hourly: [{ hour: 13, total: 7 }],
    hourly_previous: [{ hour: 13, total: 1 }],
    station_pie: stations.map((station, index) => ({
      station_id: station.id,
      total: 10 + index,
    })),
    per_station: stations.map((station, index) => ({
      station_id: station.id,
      current: [bucket(10 + index)],
      previous: [bucket(2 + index)],
      weekday_radar: [{ weekday: 3, total: 8 + index }],
      weekday_radar_previous: [{ weekday: 3, total: 1 + index }],
      hourly: [{ hour: 13, total: 10 + index }],
      hourly_previous: [{ hour: 13, total: 1 + index }],
    })),
  }
}

const MONTHLY = {
  monthly_totals: [
    { year: 2024, month: 1, total: 12 },
    { year: 2025, month: 1, total: 20 },
  ],
}

interface ServerOptions {
  shellError?: boolean
  overviewError?: boolean
  graphsError?: boolean
  monthlyError?: boolean
  emptyGraphs?: boolean
  stationCount?: number
  /** URLs (substring) that should never resolve (to hold the cards in loading). */
  pending?: string[]
}

function makeServer(options: ServerOptions = {}) {
  const stations = stationsForCount(options.stationCount ?? DEFAULT_STATIONS.length)
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (options.pending?.some((part) => url.includes(part))) {
      return new Promise<Response>(() => {})
    }
    if (url.includes('/api/bff/stations/summary')) {
      if (options.shellError) return Promise.reject(new Error('shell failed'))
      return Promise.resolve(ok(rawShell(stations)))
    }
    if (url.includes('/styles/basemap.json')) {
      return Promise.resolve(ok({ sources: {} }))
    }
    if (url.includes('/api/bff/global-summary')) {
      return Promise.resolve(ok(GLOBAL_SUMMARY))
    }
    if (url.includes('/api/graphs/summary/')) {
      if (options.graphsError) return Promise.reject(new Error('graphs failed'))
      return Promise.resolve(ok(graphsFor(stations, options.emptyGraphs ?? false)))
    }
    if (url.includes('/api/overview/summary')) {
      if (options.overviewError) return Promise.reject(new Error('overview failed'))
      return Promise.resolve(ok(OVERVIEW))
    }
    if (url.includes('/api/monthly/summary')) {
      if (options.monthlyError) return Promise.reject(new Error('monthly failed'))
      return Promise.resolve(ok(MONTHLY))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return { mock }
}

function renderSummary(entry: string) {
  return render(
    <MemoryRouter initialEntries={[entry]}>
      <TrendSettingsProvider>
        <Routes>
          <Route path="*" element={<StationsSummary />} />
        </Routes>
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

describe('StationsSummary', () => {
  it('shows the page-shell loading skeleton while the shell request is pending', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise<Response>(() => {})),
    )
    const { container } = renderSummary(ENTRY)

    expect(screen.getByRole('link', { name: 'Back to map' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Station summary' })).not.toBeInTheDocument()
    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull()
  })

  it('shows the shell error text when the shell request fails', async () => {
    makeServer({ shellError: true })
    renderSummary(ENTRY)

    expect(await screen.findByText('Could not load the station summary.')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Station summary' })).not.toBeInTheDocument()
  })

  it('hints when no map view is selected and links straight back to the map', async () => {
    makeServer()
    renderSummary('/summary?timeframe=week')

    expect(
      screen.getByText(
        'No map view selected. Go back to the map and summarize the visible stations.',
      ),
    ).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Back to map' })).toHaveAttribute('href', '/')
  })

  it('renders the loaded shell with the summary map, overview and every chart card', async () => {
    const { mock } = makeServer()
    const { container } = renderSummary(ENTRY)

    // Shell metadata.
    expect(await screen.findByRole('heading', { name: 'Station summary' })).toBeInTheDocument()
    expect(screen.getByAltText('Station summary image')).toHaveAttribute('src', '/img/summary.png')
    expect(screen.getByText('2 stations')).toBeInTheDocument()
    // The channel-count badge belongs to the overview card, so wait for it.
    expect(await screen.findByText('5 channels')).toBeInTheDocument()
    expect(screen.getByText(/^Updated /)).toBeInTheDocument()
    // The back link restores the exact map view from the URL bounds.
    expect(screen.getByRole('link', { name: 'Back to map' })).toHaveAttribute(
      'href',
      `/?${BASE_QUERY}`,
    )

    // Interactive summary map markers.
    expect(await screen.findByAltText('A-Stadt')).toBeInTheDocument()
    expect(screen.getByAltText('B-Stadt')).toBeInTheDocument()

    // Overview card: the all-time counter + metric boxes.
    expect(await screen.findByText('Total bikes (all time)')).toBeInTheDocument()
    expect(screen.getByText('1.234')).toBeInTheDocument()
    expect(screen.getByText('Last 24 hours')).toBeInTheDocument()
    expect(screen.getByText('Last 7 days')).toBeInTheDocument()

    // Detailed statistics: key facts + the aggregate bar/radar cards.
    expect(screen.getByRole('heading', { name: 'Detailed statistics' })).toBeInTheDocument()
    expect(screen.getByText('Total bikes in selection')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Detailed stats' })).toBeInTheDocument()
    expect(screen.getByText('Share by station')).toBeInTheDocument()

    // Per-station pie legend entries derived from the graphs station shares.
    expect(screen.getByText('A-Stadt · 10')).toBeInTheDocument()
    expect(screen.getByText('B-Stadt · 11')).toBeInTheDocument()

    // Monthly bar chart (standalone card at the bottom).
    expect(screen.getByRole('heading', { name: 'Bikes per month' })).toBeInTheDocument()
    expect(screen.getByText('2025')).toBeInTheDocument()

    // The mocked recharts surfaces render for the non-empty charts.
    expect(container.querySelector('[data-testid="recharts-BarChart"]')).not.toBeNull()
    expect(container.querySelector('[data-testid="recharts-RadarChart"]')).not.toBeNull()
    expect(container.querySelector('[data-testid="recharts-PieChart"]')).not.toBeNull()

    // The cards were fetched through the shell's HATEOAS links; the week graphs
    // link carries the mid-resolution hour token.
    expect(mock).toHaveBeenCalledWith(expect.stringContaining('/api/bff/stations/summary?'))
    expect(mock).toHaveBeenCalledWith('/api/overview/summary')
    expect(mock).toHaveBeenCalledWith('/api/graphs/summary/week?resolution=hour')
    expect(mock).toHaveBeenCalledWith('/api/monthly/summary')
  })

  it('shows the per-card skeletons while the card sub-requests are pending', async () => {
    makeServer({
      pending: ['/api/overview/summary', '/api/graphs/summary/', '/api/monthly/summary'],
    })
    const { container } = renderSummary(ENTRY)

    // Shell + map are loaded, so the section headings are present…
    expect(await screen.findByRole('heading', { name: 'Station summary' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Overview' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Detailed statistics' })).toBeInTheDocument()

    // …but no card content has arrived yet.
    expect(screen.queryByText('Total bikes (all time)')).not.toBeInTheDocument()
    expect(screen.queryByText('1-hour buckets')).not.toBeInTheDocument()
    expect(screen.queryByText('2025')).not.toBeInTheDocument()

    // The overview, charts, key facts and monthly skeletons are all mounted.
    const skeletonCount = container.querySelectorAll('[data-slot="skeleton"]').length
    expect(skeletonCount).toBeGreaterThan(10)
  })

  it('shows the statistics error texts when the graphs sub-resource fails', async () => {
    makeServer({ graphsError: true })
    renderSummary(ENTRY)

    expect(await screen.findByText('Could not load the statistics.')).toBeInTheDocument()
    expect(screen.getByText('Could not load the detailed stats.')).toBeInTheDocument()
    // The overview and monthly cards load independently.
    expect(screen.getByText('Total bikes (all time)')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Bikes per month' })).toBeInTheDocument()
  })

  it('shows the overview error text when the overview sub-resource fails', async () => {
    makeServer({ overviewError: true })
    renderSummary(ENTRY)

    expect(await screen.findByText('Could not load the overview.')).toBeInTheDocument()
    expect(screen.getByText('Total bikes in selection')).toBeInTheDocument()
  })

  it('shows the monthly error text when the monthly sub-resource fails', async () => {
    makeServer({ monthlyError: true })
    renderSummary(ENTRY)

    expect(await screen.findByText('Could not load the monthly totals.')).toBeInTheDocument()
  })

  it('opens the settings dialog and switching the timeframe refetches the graphs link', async () => {
    const { mock } = makeServer()
    renderSummary(ENTRY)

    await screen.findByText('Total bikes (all time)')

    fireEvent.click(screen.getByRole('button', { name: 'Calculation settings' }))
    expect(await screen.findByText('Bike-Trends settings')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'This year' }))

    // The year timeframe at the default mid resolution uses the year link with
    // the `week` granularity.
    await waitFor(() =>
      expect(mock).toHaveBeenCalledWith('/api/graphs/summary/year?resolution=week'),
    )
  })

  it('appends the Bike-Trends flag to the card requests when the link carries it', async () => {
    const { mock } = makeServer()
    renderSummary(`${ENTRY}&exclude_new_stations=1`)

    await waitFor(() =>
      expect(mock).toHaveBeenCalledWith('/api/overview/summary?exclude_new_stations=true'),
    )
    await waitFor(() =>
      expect(mock).toHaveBeenCalledWith(
        '/api/graphs/summary/week?resolution=hour&exclude_new_stations=true',
      ),
    )
    await waitFor(() =>
      expect(mock).toHaveBeenCalledWith('/api/monthly/summary?exclude_new_stations=true'),
    )
  })

  it('draws the previous-period series when compare is enabled', async () => {
    makeServer()
    const { container } = renderSummary(`${ENTRY}&compare=1`)

    await screen.findByText('Total bikes (all time)')

    const dataKeys = Array.from(container.querySelectorAll('[data-testid="recharts-Bar"]')).map(
      (bar) => {
        const props = JSON.parse(bar.getAttribute('data-recharts-props') ?? '{}') as {
          dataKey?: string
        }
        return props.dataKey
      },
    )
    // Aggregate current + previous series.
    expect(dataKeys).toContain('current')
    expect(dataKeys).toContain('previous')
    // Per-station current + previous series for each station.
    expect(dataKeys).toContain('s1_current')
    expect(dataKeys).toContain('s1_previous')
    expect(dataKeys).toContain('s2_previous')
  })

  it('shows the chart empty states when the period has no traffic yet', async () => {
    makeServer({ emptyGraphs: true })
    renderSummary(ENTRY)

    await screen.findByText('Total bikes (all time)')
    expect(screen.getAllByText('No data for this period.').length).toBeGreaterThan(0)
    expect(screen.getAllByText('No traffic for this period.').length).toBeGreaterThan(0)
    // The monthly card is not driven by the graphs and still shows its data.
    expect(screen.getByRole('heading', { name: 'Bikes per month' })).toBeInTheDocument()
  })

  it('shows the too-many-data-streams notice once many stations are selected', async () => {
    makeServer({ stationCount: 6 })
    renderSummary(ENTRY)

    await screen.findByText('Total bikes (all time)')
    expect(screen.getByText('6 stations')).toBeInTheDocument()
    expect(screen.getAllByText(/too many data-streams to render/).length).toBeGreaterThan(0)
  })

  it('excludes a station from the charts when its map flag is toggled on', async () => {
    const { mock } = makeServer()
    renderSummary(ENTRY)

    const marker = await screen.findByAltText('A-Stadt')
    expect(marker).not.toHaveClass('station-marker--disabled')

    fireEvent.click(marker)

    await waitFor(() => expect(mock).toHaveBeenCalledWith('/api/overview/summary?exclude=s1'))
    await waitFor(() =>
      expect(mock).toHaveBeenCalledWith('/api/graphs/summary/week?resolution=hour&exclude=s1'),
    )
    expect(screen.getByAltText('A-Stadt')).toHaveClass('station-marker--disabled')
  })

  it('restores an excluded station when its map flag is toggled off again', async () => {
    const { mock } = makeServer()
    renderSummary(`${ENTRY}&disabled=s1`)

    const marker = await screen.findByAltText('A-Stadt')
    await waitFor(() => expect(mock).toHaveBeenCalledWith('/api/overview/summary?exclude=s1'))
    expect(marker).toHaveClass('station-marker--disabled')

    fireEvent.click(marker)

    await waitFor(() => expect(mock).toHaveBeenCalledWith('/api/overview/summary'))
    expect(screen.getByAltText('A-Stadt')).not.toHaveClass('station-marker--disabled')
  })
})
