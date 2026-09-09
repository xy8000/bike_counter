import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Bounds } from '../../lib/geo'
import { SummaryMap } from './SummaryMap'
import type { SummaryStation } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const STATIONS: SummaryStation[] = [
  { id: 's1', name: 'A-Stadt', latitude: 51.95, longitude: 7.62, channel_count: 2 },
  { id: 's2', name: 'B-Stadt', latitude: 51.97, longitude: 7.64, channel_count: 3 },
]

const BOUNDS: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('SummaryMap', () => {
  it('renders the map container with one marker per station', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { container } = render(
      <SummaryMap stations={STATIONS} disabled={new Set()} onToggle={vi.fn()} bounds={BOUNDS} />,
    )

    expect(screen.getByLabelText('Summary map')).toBeInTheDocument()
    // The map (and its markers) mount once the basemap style sub-request resolves.
    expect(await screen.findByAltText('A-Stadt')).toBeInTheDocument()
    expect(screen.getByAltText('B-Stadt')).toBeInTheDocument()
    expect(container.querySelector('[data-testid="maplibre-Map"]')).not.toBeNull()
    expect(container.querySelectorAll('[data-testid="maplibre-Marker"]')).toHaveLength(2)
  })

  it('marks disabled stations with the grayed-out disabled flag', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    render(
      <SummaryMap
        stations={STATIONS}
        disabled={new Set(['s2'])}
        onToggle={vi.fn()}
        bounds={BOUNDS}
      />,
    )

    await screen.findByAltText('A-Stadt')
    expect(screen.getByAltText('B-Stadt')).toHaveClass('station-marker--disabled')
    expect(screen.getByAltText('A-Stadt')).not.toHaveClass('station-marker--disabled')
  })

  it('calls onToggle with the station id when its marker is clicked', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const onToggle = vi.fn()
    render(
      <SummaryMap stations={STATIONS} disabled={new Set()} onToggle={onToggle} bounds={BOUNDS} />,
    )

    await screen.findByAltText('A-Stadt')
    fireEvent.click(screen.getByAltText('A-Stadt'))

    expect(onToggle).toHaveBeenCalledTimes(1)
    expect(onToggle).toHaveBeenCalledWith('s1')
  })

  it('renders the empty map container when there are no stations', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { container } = render(
      <SummaryMap stations={[]} disabled={new Set()} onToggle={vi.fn()} bounds={BOUNDS} />,
    )

    expect(screen.getByLabelText('Summary map')).toBeInTheDocument()
    expect(await screen.findByTestId('maplibre-Map')).toBeInTheDocument()
    expect(container.querySelectorAll('[data-testid="maplibre-Marker"]')).toHaveLength(0)
  })
})
