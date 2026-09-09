import { fireEvent, render, screen, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { StationMap } from '../stations/types'
import { MapView, type PopupStationInfo } from './MapView'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const station = (overrides: Partial<StationMap> & { id: string; name: string }): StationMap => ({
  latitude: 51.9,
  longitude: 7.5,
  status: 'active',
  ...overrides,
})

/// Three stations spread far apart (≫ the cluster radius at zoom 13) so they
/// each render as their own ungrouped flag marker.
const FAR_STATIONS: StationMap[] = [
  station({ id: 'active', name: 'Active Station', latitude: 51.9, longitude: 7.5 }),
  station({ id: 'sel', name: 'Selected Station', latitude: 51.95, longitude: 7.6 }),
  station({
    id: 'inactive',
    name: 'Inactive Station',
    latitude: 51.85,
    longitude: 7.7,
    status: 'inactive',
  }),
]

const detail = (imageUrl: string, channelCount: number | null, description: string) => ({
  imageUrl,
  channelCount,
  description,
})

function detailsOf(entries: Record<string, PopupStationInfo>): Map<string, PopupStationInfo> {
  return new Map(Object.entries(entries))
}

function renderMapView(
  overrides: {
    stations?: StationMap[] | null
    selectedStationId?: string | null
    stationDetails?: Map<string, PopupStationInfo>
    error?: boolean
    statsError?: boolean
    children?: ReactNode
  } = {},
) {
  const {
    stations = FAR_STATIONS,
    selectedStationId = null,
    stationDetails = new Map(),
    error = false,
    statsError = false,
  } = overrides
  const onBounds = vi.fn()
  const onReady = vi.fn()
  const onSelectStation = vi.fn()
  const onDeselect = vi.fn()
  const utils = render(
    <MemoryRouter initialEntries={['/map']}>
      <MapView
        stations={stations}
        onBounds={onBounds}
        onReady={onReady}
        onSelectStation={onSelectStation}
        onDeselect={onDeselect}
        selectedStationId={selectedStationId}
        stationDetails={stationDetails}
        error={error}
        statsError={statsError}
      />
    </MemoryRouter>,
  )
  return { ...utils, onBounds, onReady, onSelectStation, onDeselect }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('MapView', () => {
  it('renders the base map without markers while there is no station data', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { container } = renderMapView({ stations: [] })

    await screen.findByTestId('maplibre-Map')
    expect(container.querySelectorAll('[data-testid="maplibre-Marker"]')).toHaveLength(0)
  })

  it('renders one ungrouped flag marker per visible station with the right variant class', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { container } = renderMapView({ selectedStationId: 'sel' })

    // The mocked Map fires onLoad/onMoveEnd after mount, so the cluster index
    // builds and the unclustered flags render.
    const activeImg = await screen.findByAltText('Active Station')
    expect(activeImg).toHaveClass('station-marker')
    expect(activeImg).not.toHaveClass('station-marker--selected')
    expect(activeImg).not.toHaveClass('station-marker--inactive')

    expect(screen.getByAltText('Selected Station')).toHaveClass('station-marker--selected')
    expect(screen.getByAltText('Inactive Station')).toHaveClass('station-marker--inactive')

    expect(container.querySelectorAll('[data-testid="maplibre-Marker"]')).toHaveLength(3)
    expect(container.querySelectorAll('.station-cluster')).toHaveLength(0)
  })

  it('reports the map viewport via onBounds and the ready map via onReady', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { onReady, onBounds } = renderMapView()

    await screen.findByTestId('maplibre-Map')
    await screen.findByAltText('Active Station')

    expect(onReady).toHaveBeenCalledTimes(1)
    expect(onBounds).toHaveBeenCalledWith({ min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 })
  })

  it('opens a shell-loading popup skeleton when a marker with no details is clicked', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { onSelectStation } = renderMapView({})

    fireEvent.click(await screen.findByAltText('Active Station'))

    // The popup carries the station name + the detail link while the shell
    // fields still render as skeletons.
    expect(onSelectStation).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'active', name: 'Active Station' }),
    )
    const popup = await screen.findByTestId('maplibre-Popup')
    expect(within(popup).getByRole('link', { name: 'Active Station' })).toHaveAttribute(
      'href',
      '/stations/active',
    )
    expect(popup.querySelector('[aria-busy="true"]')).not.toBeNull()
    expect(popup.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0)
  })

  it('renders the full popup (image, name, badge, description) when the details are loaded', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    renderMapView({
      stationDetails: detailsOf({
        sel: detail('/img/selected.png', 2, 'Selected station description'),
      }),
    })

    fireEvent.click(await screen.findByAltText('Selected Station'))

    const popup = await screen.findByTestId('maplibre-Popup')
    expect(within(popup).getByRole('link', { name: 'Selected Station' })).toHaveAttribute(
      'href',
      '/stations/sel',
    )
    expect(popup.querySelector('img[src="/img/selected.png"]')).not.toBeNull()
    expect(within(popup).getByText('2 channels')).toBeInTheDocument()
    expect(within(popup).getByText('Selected station description')).toBeInTheDocument()
    expect(popup.querySelector('[aria-busy="true"]')).toBeNull()
    expect(popup.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0)
  })

  it('shows a stats-loading skeleton badge while the channel count is unknown', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    renderMapView({
      stationDetails: detailsOf({ inactive: detail('/img/inactive.png', null, 'Old station') }),
    })

    fireEvent.click(await screen.findByAltText('Inactive Station'))

    const popup = await screen.findByTestId('maplibre-Popup')
    // The shell image + description are known; only the channel badge waits.
    expect(popup.querySelector('img[src="/img/inactive.png"]')).not.toBeNull()
    expect(within(popup).getByText('Old station')).toBeInTheDocument()
    expect(popup.querySelector('[aria-busy="true"]')).not.toBeNull()
    expect(popup.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0)
  })

  it('omits the skeletons when the shell errored and no details are available', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    renderMapView({ error: true })

    fireEvent.click(await screen.findByAltText('Active Station'))

    const popup = await screen.findByTestId('maplibre-Popup')
    expect(within(popup).getByRole('link', { name: 'Active Station' })).toBeInTheDocument()
    expect(popup.querySelector('[aria-busy="true"]')).toBeNull()
    expect(popup.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(0)
    expect(popup.querySelector('img')).toBeNull()
  })

  it('switches the open popup when a second marker is clicked', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    renderMapView({})

    fireEvent.click(await screen.findByAltText('Active Station'))
    await screen.findByTestId('maplibre-Popup')
    fireEvent.click(screen.getByAltText('Selected Station'))

    const popup = await screen.findByTestId('maplibre-Popup')
    expect(within(popup).getByRole('link', { name: 'Selected Station' })).toBeInTheDocument()
    expect(within(popup).queryByRole('link', { name: 'Active Station' })).not.toBeInTheDocument()
  })

  it('groups stations closer than the cluster radius into a numbered circle that zooms in on click', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const clustered: StationMap[] = [
      station({ id: 'c1', name: 'Clustered One', latitude: 51.96, longitude: 7.63 }),
      station({ id: 'c2', name: 'Clustered Two', latitude: 51.9601, longitude: 7.6301 }),
    ]
    const { onReady, onSelectStation } = renderMapView({ stations: clustered })

    // Two close stations collapse into one cluster circle (not two flags).
    const circle = await screen.findByRole('button', { name: '2 stations' })
    expect(circle).toHaveAttribute('data-count', '2')
    expect(screen.queryByAltText('Clustered One')).not.toBeInTheDocument()

    const map = onReady.mock.calls[0]?.[0] as { easeTo: ReturnType<typeof vi.fn> } | undefined

    fireEvent.click(circle)

    // Clicking a circle eases the map to the cluster's expansion zoom; it does
    // not select a station.
    expect(map?.easeTo).toHaveBeenCalledTimes(1)
    expect(onSelectStation).not.toHaveBeenCalled()
    expect(screen.queryByTestId('maplibre-Popup')).not.toBeInTheDocument()
  })
})
