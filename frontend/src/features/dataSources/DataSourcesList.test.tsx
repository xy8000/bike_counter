import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import { DataSourcesList, dataSourceImageUrl } from './DataSourcesList'
import type { DataSourceSummary } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const GLOBAL_SUMMARY = {
  station_count: 4,
  channel_count: 8,
  bikes_last_day_total: 120,
  last_update: null,
}

const ITEMS: DataSourceSummary[] = [
  {
    id: 'ms',
    name: 'Münster',
    provider_type: 'radvis',
    last_updated_at: '2026-01-02T12:00:00Z',
    station_count: 3,
    channel_count: 6,
    image_url: '/logos/ms.png',
    last_import: {
      status: 'FINISHED',
      started_at: '2026-01-02T10:00:00Z',
      finished_at: '2026-01-02T10:01:00Z',
      duration_seconds: 60,
      failure_message: null,
      warning_count: 0,
      error_count: 0,
    },
  },
  {
    id: 'bonn',
    name: 'Bonn',
    provider_type: 'open_data',
    last_updated_at: null,
    station_count: 1,
    channel_count: 2,
    image_url: '',
    last_import: null,
  },
]

function makeServer(
  options: { error?: boolean; pending?: boolean; items?: DataSourceSummary[] } = {},
) {
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (options.pending) return new Promise<Response>(() => {})
    if (options.error) return Promise.reject(new Error('list failed'))
    if (url.includes('/api/bff/data-sources')) {
      return Promise.resolve(ok({ items: options.items ?? ITEMS }))
    }
    if (url.includes('/api/bff/global-summary')) {
      return Promise.resolve(ok(GLOBAL_SUMMARY))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return mock
}

function LocationProbe() {
  const location = useLocation()
  return <div data-testid="location">{location.pathname}</div>
}

function renderList() {
  return render(
    <MemoryRouter initialEntries={['/data-sources']}>
      <TrendSettingsProvider>
        <Routes>
          <Route path="/data-sources" element={<DataSourcesList />} />
          <Route path="*" element={null} />
        </Routes>
        <LocationProbe />
      </TrendSettingsProvider>
    </MemoryRouter>,
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('dataSourceImageUrl', () => {
  it('passes through the served logo and falls back to the bundled svg', () => {
    expect(dataSourceImageUrl('/logos/ms.png')).toBe('/logos/ms.png')
    expect(dataSourceImageUrl('')).toBeTruthy()
    expect(dataSourceImageUrl('')).not.toBe('/logos/ms.png')
  })
})

describe('DataSourcesList', () => {
  it('shows the heading and the loading skeleton while the list is pending', () => {
    makeServer({ pending: true })
    const { container } = renderList()

    expect(screen.getByRole('heading', { name: 'Data sources' })).toBeInTheDocument()
    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull()
    expect(screen.queryByText('Münster')).not.toBeInTheDocument()
  })

  it('shows the error text when the list request fails', async () => {
    makeServer({ error: true })
    renderList()

    expect(await screen.findByText('Could not load the data sources.')).toBeInTheDocument()
  })

  it('shows the empty state when no data sources are configured', async () => {
    makeServer({ items: [] })
    renderList()

    expect(await screen.findByText('No data sources configured.')).toBeInTheDocument()
    expect(screen.queryByText('Münster')).not.toBeInTheDocument()
  })

  it('renders a row per data source with its status, counts and logo fallback', async () => {
    makeServer()
    renderList()

    // The name is duplicated by the desktop + mobile layouts of each row.
    expect((await screen.findAllByText('Münster')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('Bonn').length).toBeGreaterThan(0)

    // Import status for the last import of each source.
    expect(screen.getAllByText('Succeeded').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Never imported').length).toBeGreaterThan(0)

    // Counts rendered for both rows.
    expect(screen.getAllByText('3').length).toBeGreaterThan(0)
    expect(screen.getAllByText('6').length).toBeGreaterThan(0)

    // A real logo is served for Münster; Bonn falls back to the bundled svg.
    const muesterImages = screen.getAllByAltText('Münster image')
    expect(muesterImages[0]).toHaveAttribute('src', '/logos/ms.png')
    const bonnImages = screen.getAllByAltText('Bonn image')
    expect(bonnImages[0]).toHaveAttribute('src', dataSourceImageUrl(''))
  })

  it('navigates to the detail page when a row is clicked', async () => {
    makeServer()
    renderList()

    const row = (await screen.findAllByText('Münster'))[0].closest('a')
    expect(row).not.toBeNull()
    expect(row).toHaveAttribute('href', '/data-sources/ms')

    fireEvent.click(row as HTMLElement)

    await waitFor(() => expect(screen.getByTestId('location').textContent).toBe('/data-sources/ms'))
  })
})
