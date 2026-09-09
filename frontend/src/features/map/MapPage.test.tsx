import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import type {
  SidebarShell,
  SidebarStation,
  SidebarStationStats,
  StationMap,
} from '../stations/types'
import MapPage from './MapPage'

const SIDEBAR_STATS_URL =
  '/api/bff/stations/sidebar/stats?min_lat=51&min_lng=7&max_lat=52&max_lng=8'

const MARKERS: StationMap[] = [
  { id: 's1', name: 'Station Alpha', latitude: 51.9, longitude: 7.5, status: 'active' },
  { id: 's2', name: 'Station Beta', latitude: 51.95, longitude: 7.6, status: 'inactive' },
]

// The sidebar shell uses null coordinates so clicking an item never reaches
// MapLibre's flyTo (the fake map exposes no flyTo under jsdom).
const SHELL_ITEMS: SidebarStation[] = [
  {
    id: 's1',
    name: 'Station Alpha',
    description: 'Alpha description',
    latitude: null,
    longitude: null,
    image_url: '/img/alpha.png',
  },
  {
    id: 's2',
    name: 'Station Beta',
    description: 'Beta description',
    latitude: null,
    longitude: null,
    image_url: '/img/beta.png',
  },
]

const SIDEBAR_STATS: SidebarStationStats[] = [
  { station_id: 's1', channel_count: 2, bikes_last_day: 30 },
  { station_id: 's2', channel_count: 1, bikes_last_day: 5 },
]

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function shellPayload(): SidebarShell {
  return {
    items: SHELL_ITEMS,
    visible_count: 2,
    total_count: 5,
    _links: { stats: SIDEBAR_STATS_URL },
  }
}

function rawShellPayload() {
  return {
    id: 's1',
    name: 'Station Alpha',
    description: 'Alpha description',
    latitude: 51.9,
    longitude: 7.5,
    channel_count: 2,
    image_url: '/img/alpha.png',
    last_update: '2026-01-02T12:00:00Z',
    detail_url: '/stations/s1',
    _links: { stats: { href: '/api/bff/station-overview/s1/stats', templated: false } },
  }
}

function overviewStatsPayload() {
  return {
    total_bikes: 123,
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
}

function basemapPayload() {
  return {
    version: 8,
    sources: {
      protomaps: {
        type: 'vector',
        url: 'pmtiles://REPLACED_AT_RUNTIME/tiles/map.pmtiles',
      },
    },
  }
}

/// One fake fetch serving every endpoint MapPage's subtree hits: the basemap
/// style (BaseMap), the global summary (TopBar), the visible stations + shell +
/// stats (useVisibleStations) and the station-overview shell + stats.
function makeServer(options: { failStations?: boolean } = {}) {
  const state = { failStations: options.failStations ?? false }
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (url.includes('/styles/basemap')) return Promise.resolve(ok(basemapPayload()))
    if (url.includes('/global-summary')) {
      return Promise.resolve(
        ok({ station_count: 4, channel_count: 8, bikes_last_day_total: 120, last_update: null }),
      )
    }
    if (url.includes('/station-overview/')) {
      if (url.endsWith('/stats')) return Promise.resolve(ok(overviewStatsPayload()))
      return Promise.resolve(ok(rawShellPayload()))
    }
    if (url.includes('/stations/sidebar/stats')) {
      return Promise.resolve(ok({ items: SIDEBAR_STATS }))
    }
    if (url.includes('/stations/sidebar')) {
      if (state.failStations) return Promise.reject(new Error('shell failed'))
      return Promise.resolve(
        ok({ ...shellPayload(), _links: { stats: { href: SIDEBAR_STATS_URL, templated: false } } }),
      )
    }
    if (url.includes('/stations?')) {
      if (state.failStations) return Promise.reject(new Error('markers failed'))
      return Promise.resolve(ok({ items: MARKERS }))
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

function locationText(): string {
  return screen.getByTestId('location').textContent ?? ''
}

type MatchMediaMock = ReturnType<typeof vi.fn>

/// Point window.matchMedia at a predicate so the initial map/sidebar state can
/// be driven (e.g. desktop `min-width: 640px` matches, phones never match).
function stubMatchMedia(predicate: (query: string) => boolean) {
  ;(window.matchMedia as unknown as MatchMediaMock).mockImplementation((query: string) => ({
    matches: predicate(query),
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }))
}

const desktopMedia = () => stubMatchMedia((query) => query.includes('min-width: 640px'))
const phoneMedia = () => stubMatchMedia(() => false)

const restoreMedia = () => stubMatchMedia(() => false)

function renderMapPage(initialEntry = '/') {
  const utils = render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <TrendSettingsProvider>
        <MapPage />
      </TrendSettingsProvider>
      <LocationProbe />
    </MemoryRouter>,
  )
  return utils
}

/// The phone-only floating button ("Show station list") is rendered outside the
/// sliding left panel; the SidebarHandle shares the label but lives inside the
/// <aside>, so this picks the actual FAB.
function fabButton(): HTMLElement | null {
  return (
    Array.from(document.querySelectorAll('button')).find(
      (button) =>
        button.getAttribute('aria-label') === 'Show station list' && !button.closest('aside'),
    ) ?? null
  )
}

const WAIT = { timeout: 3000 }

afterEach(() => {
  vi.unstubAllGlobals()
  restoreMedia()
})

describe('MapPage', () => {
  it('renders the station-list sidebar, the header and the map markers on a desktop URL without a station', async () => {
    desktopMedia()
    makeServer()
    renderMapPage('/')

    // Header (shared SearchableHeader) is present.
    expect(screen.getByRole('link', { name: 'Bike Counter' })).toBeInTheDocument()

    // Sidebar (default expanded on desktop) lists the visible stations once the
    // debounced shell fetch lands.
    expect(await screen.findByText('Visible counting stations', {}, WAIT)).toBeInTheDocument()
    expect(await screen.findByText('2 / 5', {}, WAIT)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Station Alpha/ })).toBeInTheDocument()

    // The map reports its viewport and the marker flags render.
    expect(await screen.findByAltText('Station Alpha', {}, WAIT)).toBeInTheDocument()
    expect(screen.getByAltText('Station Beta')).toHaveClass('station-marker--inactive')

    // Desktop default is expanded: no floating toggle.
    expect(fabButton()).toBeNull()
  })

  it('starts collapsed on phones and toggles the sidebar with the H key and the FAB', async () => {
    phoneMedia()
    makeServer()
    renderMapPage('/')

    // Phones default to collapsed: the floating toggle is present.
    expect(fabButton()).not.toBeNull()

    // H collapses/expands.
    fireEvent.keyDown(window, { key: 'h' })
    expect(fabButton()).toBeNull()

    fireEvent.keyDown(window, { key: 'H' })
    expect(fabButton()).not.toBeNull()

    // The FAB re-expands the drawer.
    fireEvent.click(fabButton() as HTMLElement)
    expect(fabButton()).toBeNull()
  })

  it('opens the station overview when a sidebar station is selected and syncs the URL', async () => {
    desktopMedia()
    makeServer()
    renderMapPage('/')

    const item = await screen.findByRole('button', { name: /Station Alpha/ }, { timeout: 3000 })
    fireEvent.click(item)

    // The sidebar is replaced by the overview panel.
    expect(await screen.findByRole('link', { name: 'Station Alpha' }, WAIT)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close station overview' })).toBeInTheDocument()
    expect(screen.queryByText('Visible counting stations')).not.toBeInTheDocument()

    // The URL mirrors the open overview (station param + serialized bounds).
    expect(locationText()).toContain('station=s1')
    expect(locationText()).toContain('min_lat=51')
  })

  it('renders the overview panel directly for a shared URL with ?station=', async () => {
    desktopMedia()
    makeServer()
    renderMapPage('/map?station=s1')

    // The list sidebar is not rendered; the overview loads for station s1.
    expect(await screen.findByRole('link', { name: 'Station Alpha' }, WAIT)).toBeInTheDocument()
    expect(screen.queryByText('Visible counting stations')).not.toBeInTheDocument()
    expect(locationText()).toContain('station=s1')
  })

  it('navigates to /summary with the serialized bounds via the summarize button', async () => {
    desktopMedia()
    makeServer()
    renderMapPage('/')

    await screen.findByRole('button', { name: /Station Alpha/ }, { timeout: 3000 })
    const summarize = screen.getByRole('button', { name: /Summarize visible stations/ })
    expect(summarize).toBeEnabled()

    fireEvent.click(summarize)

    await waitFor(() => {
      expect(locationText()).toContain('/summary')
    }, WAIT)
    expect(locationText()).toContain('min_lat=51')
    expect(locationText()).toContain('min_lng=7')
    expect(locationText()).toContain('max_lat=52')
    expect(locationText()).toContain('max_lng=8')
  })

  it('surfaces a visible-stations fetch error in the sidebar', async () => {
    desktopMedia()
    makeServer({ failStations: true })
    renderMapPage('/')

    expect(
      await screen.findByText('Could not load counting stations.', {}, WAIT),
    ).toBeInTheDocument()
  })
})
