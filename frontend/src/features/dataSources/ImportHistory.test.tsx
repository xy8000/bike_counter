import { ok } from '@/test-utils/http'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { IMPORT_POLL_INTERVAL_MS } from './api'
import { ImportHistory } from './ImportHistory'
import type { DataSourceImport, DataSourceImports } from './types'

const RUNS: DataSourceImports = {
  total: 2,
  offset: 0,
  limit: 50,
  items: [
    {
      status: 'FINISHED',
      started_at: '2026-01-02T10:00:00Z',
      finished_at: '2026-01-02T11:00:00Z',
      duration_seconds: 3600,
      failure_message: null,
      warning_count: 1,
      error_count: 0,
      added_measurements: 120,
      added_stations: 2,
    },
    {
      status: 'FAILED',
      started_at: '2026-01-01T10:00:00Z',
      finished_at: '2026-01-01T10:01:00Z',
      duration_seconds: 60,
      failure_message: 'boom',
      warning_count: 0,
      error_count: 3,
      added_measurements: 0,
      added_stations: 0,
    },
  ],
  average_duration_seconds: 1830,
}

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('ImportHistory', () => {
  it('renders the jobs table with the run facts and the average duration', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(RUNS))
    vi.stubGlobal('fetch', fetchMock)
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('Import jobs')).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/data-sources/ms/imports?offset=0&limit=50')
    expect(screen.getByText('Showing 1-2 of 2 import jobs')).toBeInTheDocument()
    expect(screen.getByText('Page 1 of 1')).toBeInTheDocument()
    expect(screen.getByText(/Average import duration:/)).toBeInTheDocument()
    expect(screen.getByText('30m 30s')).toBeInTheDocument()

    // Column headers.
    for (const header of [
      'Start time',
      'Duration',
      'Status',
      'Warnings',
      'Errors',
      'New measurements',
      'New stations',
    ]) {
      expect(screen.getByRole('columnheader', { name: header })).toBeInTheDocument()
    }

    // Row values.
    expect(screen.getByText('1h 0m')).toBeInTheDocument()
    expect(screen.getByText('FINISHED')).toBeInTheDocument()
    expect(screen.getByText('FAILED')).toBeInTheDocument()
    expect(screen.getByText('120')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
  })

  it('polls a running run and updates its live counters until it finishes', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    const running: DataSourceImport = {
      status: 'RUNNING',
      started_at: new Date().toISOString(),
      finished_at: null,
      duration_seconds: null,
      failure_message: null,
      warning_count: 1,
      error_count: 2,
      added_measurements: 3,
      added_stations: 4,
    }
    const finished: DataSourceImport = {
      ...running,
      status: 'FINISHED',
      finished_at: new Date().toISOString(),
      duration_seconds: 30,
      warning_count: 5,
      error_count: 6,
      added_measurements: 7,
      added_stations: 8,
    }
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        ok({ items: [running], total: 1, offset: 0, limit: 50, average_duration_seconds: null }),
      )
      .mockResolvedValueOnce(
        ok({ items: [finished], total: 1, offset: 0, limit: 50, average_duration_seconds: 30 }),
      )
    vi.stubGlobal('fetch', fetchMock)
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('RUNNING')).toBeInTheDocument()
    expect(screen.getByText('3')).toBeInTheDocument()
    expect(screen.getByText('4')).toBeInTheDocument()

    await act(async () => {
      vi.advanceTimersByTime(IMPORT_POLL_INTERVAL_MS)
    })

    expect(await screen.findByText('FINISHED')).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(screen.getByText('7')).toBeInTheDocument()
    expect(screen.getByText('8')).toBeInTheDocument()

    // Once no run is RUNNING the interval is cleared and no further poll happens.
    await act(async () => {
      vi.advanceTimersByTime(IMPORT_POLL_INTERVAL_MS)
    })
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('shows the empty state when there are no runs yet', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          ok({ items: [], total: 0, offset: 0, limit: 50, average_duration_seconds: null }),
        ),
    )
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('No import jobs recorded yet.')).toBeInTheDocument()
    expect(screen.queryByText(/Average import duration:/)).not.toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'Import history pagination' })).toBeNull()
  })

  it('shows an error message when the history request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('Could not load the import jobs.')).toBeInTheDocument()
  })

  it('pages through the history with Previous and Next', async () => {
    const firstPage = page(0, 50, 55)
    const secondPage = page(50, 5, 55)
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok(firstPage))
      .mockResolvedValueOnce(ok(secondPage))
      .mockResolvedValueOnce(ok(firstPage))
    vi.stubGlobal('fetch', fetchMock)
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('Showing 1-50 of 55 import jobs')).toBeInTheDocument()
    expect(screen.getByText('Page 1 of 2')).toBeInTheDocument()
    expect(screen.getAllByTestId('import-run-row')).toHaveLength(50)
    expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Next' })).toBeEnabled()

    fireEvent.click(screen.getByRole('button', { name: 'Next' }))

    expect(await screen.findByText('Showing 51-55 of 55 import jobs')).toBeInTheDocument()
    expect(screen.getByText('Page 2 of 2')).toBeInTheDocument()
    expect(screen.getAllByTestId('import-run-row')).toHaveLength(5)
    expect(screen.getByRole('button', { name: 'Previous' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled()

    fireEvent.click(screen.getByRole('button', { name: 'Previous' }))

    expect(await screen.findByText('Showing 1-50 of 55 import jobs')).toBeInTheDocument()
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/bff/data-sources/ms/imports?offset=0&limit=50',
      '/api/bff/data-sources/ms/imports?offset=50&limit=50',
      '/api/bff/data-sources/ms/imports?offset=0&limit=50',
    ])
  })

  it('restarts at the first page when the data source changes', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(page(0, 50, 55)))
    vi.stubGlobal('fetch', fetchMock)
    const { rerender } = render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('Showing 1-50 of 55 import jobs')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() =>
      expect(fetchMock).toHaveBeenLastCalledWith(
        '/api/bff/data-sources/ms/imports?offset=50&limit=50',
      ),
    )

    rerender(<ImportHistory dataSourceId="bonn" />)

    await waitFor(() =>
      expect(fetchMock).toHaveBeenLastCalledWith(
        '/api/bff/data-sources/bonn/imports?offset=0&limit=50',
      ),
    )
  })
})

/// Builds one page of synthetic finished runs for the pagination tests.
function page(offset: number, count: number, total: number): DataSourceImports {
  const items: DataSourceImport[] = Array.from({ length: count }, (_, index) => {
    const minute = String(index).padStart(2, '0')
    return {
      status: 'FINISHED',
      started_at: `2026-01-01T00:${minute}:00Z`,
      finished_at: `2026-01-01T01:${minute}:00Z`,
      duration_seconds: 3600,
      failure_message: null,
      warning_count: 0,
      error_count: 0,
      added_measurements: 0,
      added_stations: 0,
    }
  })
  return { items, total, offset, limit: 50, average_duration_seconds: 3600 }
}
