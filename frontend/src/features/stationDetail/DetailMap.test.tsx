import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'
import { DetailMap } from './DetailMap'

// BaseMap lives in another feature (map/), which fetches its basemap style and
// wires the real MapLibre click handler. DetailMap's job is only to forward the
// station coords + openMap callback, so stub BaseMap to render its children and
// fire the void-click callback with a deterministic fake map (bounds 51/7/52/8,
// mirroring the shared maplibre fake map).
vi.mock('../map/BaseMap', async () => {
  const React = (await import('react')) as typeof import('react')
  return {
    BaseMap: (props: {
      children?: ReactNode
      interactive?: boolean
      initialViewState?: { longitude: number; latitude: number; zoom: number }
      onVoidClick?: (map: { getBounds: () => unknown }) => void
    }) => {
      React.useEffect(() => {
        props.onVoidClick?.({
          getBounds: () => ({
            getSouth: () => 51,
            getWest: () => 7,
            getNorth: () => 52,
            getEast: () => 8,
          }),
        })
      }, [])
      return (
        <div
          data-testid="basemap"
          data-interactive={String(props.interactive)}
          data-view={JSON.stringify(props.initialViewState)}
        >
          {props.children}
        </div>
      )
    },
  }
})

function LocationProbe() {
  const location = useLocation()
  return (
    <div data-testid="location">
      {location.pathname}
      {location.search}
    </div>
  )
}

function renderDetailMap(props: { latitude: number | null; longitude: number | null }) {
  const utils = render(
    <MemoryRouter initialEntries={['/stations/s1']}>
      <DetailMap latitude={props.latitude} longitude={props.longitude} name="Zoo Station" />
      <LocationProbe />
    </MemoryRouter>,
  )
  return { ...utils, location: () => screen.getByTestId('location').textContent }
}

describe('DetailMap', () => {
  it('shows the fallback message when the station has no coordinates', () => {
    renderDetailMap({ latitude: null, longitude: null })

    expect(screen.getByText('No map position available for this station.')).toBeInTheDocument()
    expect(screen.queryByTestId('basemap')).not.toBeInTheDocument()
  })

  it('renders a non-interactive preview centred on the station with a marker', () => {
    const { container } = renderDetailMap({ latitude: 51.96, longitude: 7.63 })

    const basemap = screen.getByTestId('basemap')
    expect(basemap).toHaveAttribute('data-interactive', 'false')
    expect(basemap).toHaveAttribute(
      'data-view',
      JSON.stringify({ longitude: 7.63, latitude: 51.96, zoom: 15 }),
    )

    const marker = container.querySelector('[data-testid="maplibre-Marker"]')
    expect(marker).not.toBeNull()
    expect(screen.getByAltText('Zoo Station')).toBeInTheDocument()
  })

  it('navigates to the serialized visible bounds when the map is void-clicked', () => {
    renderDetailMap({ latitude: 51.96, longitude: 7.63 })

    const location = screen.getByTestId('location').textContent
    expect(location).toBe('/?min_lat=51&min_lng=7&max_lat=52&max_lng=8')
  })
})
