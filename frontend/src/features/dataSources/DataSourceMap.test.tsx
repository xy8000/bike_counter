import { render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DataSourceMap } from './DataSourceMap'
import type { DataSourceMapStation } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const STATIONS: DataSourceMapStation[] = [
  { id: 'st1', name: 'A-Station', latitude: 51.96, longitude: 7.63, status: 'active' },
  { id: 'st2', name: 'B-Station', latitude: 51.97, longitude: 7.64, status: 'inactive' },
]

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('DataSourceMap', () => {
  it('shows a placeholder when there are no positioned stations', () => {
    render(<DataSourceMap stations={[]} name="Münster" />)

    expect(
      screen.getByText('No positioned stations available for this data source.'),
    ).toBeInTheDocument()
  })

  it('renders the map with one marker per station', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    const { container } = render(<DataSourceMap stations={STATIONS} name="Münster" />)

    expect(screen.getByLabelText('Münster stations map')).toBeInTheDocument()
    expect(await screen.findByAltText('A-Station')).toBeInTheDocument()
    expect(screen.getByAltText('B-Station')).toBeInTheDocument()
    expect(container.querySelector('[data-testid="maplibre-Map"]')).not.toBeNull()
    expect(container.querySelectorAll('[data-testid="maplibre-Marker"]')).toHaveLength(2)
  })

  it('marks inactive stations with the inactive flag class', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok({ sources: {} })))
    render(<DataSourceMap stations={STATIONS} name="Münster" />)

    await screen.findByAltText('A-Station')
    expect(screen.getByAltText('B-Station')).toHaveClass('station-marker--inactive')
    expect(screen.getByAltText('A-Station')).not.toHaveClass('station-marker--inactive')
  })
})
