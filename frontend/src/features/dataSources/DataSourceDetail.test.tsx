import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import { DataSourceDetail } from './DataSourceDetail'
import type { DataSourceDetail as DataSourceDetailType } from './types'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

const GLOBAL_SUMMARY = {
  station_count: 4,
  channel_count: 8,
  bikes_last_day_total: 120,
  last_update: null,
}

function detail(overrides: Partial<DataSourceDetailType>): DataSourceDetailType {
  return {
    id: 'ms',
    name: 'Münster',
    provider_type: 'radvis',
    image_url: '/logos/ms.png',
    station_count: 3,
    channel_count: 6,
    stations: [
      { id: 'st1', name: 'A-Station', latitude: 51.96, longitude: 7.63, status: 'active' },
      { id: 'st2', name: 'B-Station', latitude: 51.97, longitude: 7.64, status: 'inactive' },
    ],
    last_updated_at: '2026-01-02T12:00:00Z',
    imported_until: '2026-01-02T11:00:00Z',
    first_data_at: '2020-01-01T00:00:00Z',
    last_data_at: '2026-01-02T11:00:00Z',
    has_historical: true,
    has_real_time: true,
    has_full_current_year: false,
    last_import: {
      status: 'FINISHED',
      started_at: '2026-01-02T10:00:00Z',
      finished_at: '2026-01-02T11:02:05Z',
      duration_seconds: 3725,
      failure_message: null,
      warning_count: 2,
      error_count: 0,
    },
    ...overrides,
  }
}

function makeServer(
  options: { error?: boolean; pending?: boolean; payload?: DataSourceDetailType } = {},
) {
  const mock = vi.fn((input: unknown): Promise<Response> => {
    const url = String(input)
    if (options.pending) return new Promise<Response>(() => {})
    if (options.error) return Promise.reject(new Error('detail failed'))
    if (url.includes('/api/bff/data-sources/ms')) {
      return Promise.resolve(ok(options.payload ?? detail({})))
    }
    // The BaseMap tiles-readiness probe: the archive is served so the preview
    // map mounts under jsdom.
    if (url.includes('/tiles/map.pmtiles')) {
      return Promise.resolve(new Response(null, { status: 200 }))
    }
    if (url.includes('/styles/basemap.json')) {
      return Promise.resolve(ok({ sources: {} }))
    }
    if (url.includes('/api/bff/global-summary')) {
      return Promise.resolve(ok(GLOBAL_SUMMARY))
    }
    return Promise.reject(new Error(`unexpected url: ${url}`))
  })
  vi.stubGlobal('fetch', mock)
  return mock
}

function renderDetail() {
  return render(
    <MemoryRouter initialEntries={['/data-sources/ms']}>
      <TrendSettingsProvider>
        <Routes>
          <Route path="/data-sources/:dataSourceId" element={<DataSourceDetail />} />
          <Route path="*" element={null} />
        </Routes>
      </TrendSettingsProvider>
    </MemoryRouter>,
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('DataSourceDetail', () => {
  it('shows the loading skeleton while the detail request is pending', () => {
    makeServer({ pending: true })
    const { container } = renderDetail()

    expect(screen.getByRole('link', { name: 'Back to data sources' })).toBeInTheDocument()
    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull()
    expect(screen.queryByRole('heading', { name: 'Münster' })).not.toBeInTheDocument()
  })

  it('shows the error text when the detail request fails', async () => {
    makeServer({ error: true })
    renderDetail()

    expect(await screen.findByText('Could not load the data source.')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Münster' })).not.toBeInTheDocument()
  })

  it('renders the loaded detail with its map, badges and stat cards', async () => {
    makeServer()
    renderDetail()

    // Image + name + provider.
    expect(await screen.findByRole('heading', { name: 'Münster' })).toBeInTheDocument()
    expect(screen.getByAltText('Münster image')).toHaveAttribute('src', '/logos/ms.png')
    expect(screen.getByText('radvis')).toBeInTheDocument()
    expect(screen.getByText(/^Updated /)).toBeInTheDocument()

    // Feature badges: available + missing capability.
    expect(screen.getByText('Historical data')).toBeInTheDocument()
    expect(screen.getByText('Real-time data')).toBeInTheDocument()
    expect(screen.getByText('No full current year coverage')).toBeInTheDocument()

    // The provided stations render as map markers once the basemap style loads.
    expect(screen.getByLabelText('Münster stations map')).toBeInTheDocument()
    expect(await screen.findByAltText('A-Station')).toBeInTheDocument()
    expect(screen.getByAltText('B-Station')).toBeInTheDocument()

    // Data-Overview stat cards.
    expect(screen.getByRole('heading', { name: 'Data overview' })).toBeInTheDocument()
    expect(screen.getByText('Stations')).toBeInTheDocument()
    expect(screen.getByText('Channels')).toBeInTheDocument()
    expect(screen.getByText('First data from')).toBeInTheDocument()
    expect(screen.getByText('Data imported until')).toBeInTheDocument()
    expect(screen.getByText('Last import')).toBeInTheDocument()
    expect(screen.getByText('Import duration')).toBeInTheDocument()
    expect(screen.getByText('Warnings (last import)')).toBeInTheDocument()
    expect(screen.getByText('Errors (last import)')).toBeInTheDocument()

    // Last import status + the formatted duration (3725s = 1h 2m).
    expect(screen.getByText('Succeeded')).toBeInTheDocument()
    expect(screen.getByText('1h 2m')).toBeInTheDocument()
  })

  it('surfaces a failed last import with its failure message', async () => {
    makeServer({
      payload: detail({
        last_import: {
          status: 'FAILED',
          started_at: '2026-01-02T10:00:00Z',
          finished_at: '2026-01-02T10:01:00Z',
          duration_seconds: 60,
          failure_message: 'Connection refused',
          warning_count: 0,
          error_count: 3,
        },
      }),
    })
    renderDetail()

    expect(await screen.findByRole('heading', { name: 'Münster' })).toBeInTheDocument()
    // The failure message is concatenated into the alert paragraph.
    expect(screen.getByText(/The last import failed: Connection refused/)).toBeInTheDocument()
    expect(screen.getByText('Failed')).toBeInTheDocument()
  })

  it('shows an unknown import duration for a run that has not finished', async () => {
    makeServer({
      payload: detail({
        last_import: {
          status: 'RUNNING',
          started_at: new Date(Date.now() - 5_000).toISOString(),
          finished_at: null,
          duration_seconds: null,
          failure_message: null,
          warning_count: 0,
          error_count: 0,
        },
      }),
    })
    renderDetail()

    expect(await screen.findByRole('heading', { name: 'Münster' })).toBeInTheDocument()
    expect(screen.getByText(/Running/)).toBeInTheDocument()
    expect(screen.getByText('Unknown')).toBeInTheDocument()
  })

  it('shows "Never imported" and a dash when there is no last import', async () => {
    makeServer({ payload: detail({ last_import: null }) })
    renderDetail()

    expect(await screen.findByRole('heading', { name: 'Münster' })).toBeInTheDocument()
    expect(screen.getByText('Never imported')).toBeInTheDocument()
    expect(screen.getByText('—')).toBeInTheDocument()
  })
})
