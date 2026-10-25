import { ok } from '@/test-utils/http'
import { render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ImportHistory } from './ImportHistory'
import type { DataSourceImports } from './types'

const RUNS: DataSourceImports = {
  items: [
    {
      status: 'FINISHED',
      successful: true,
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
      successful: false,
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
  vi.unstubAllGlobals()
})

describe('ImportHistory', () => {
  it('renders the jobs table with the run facts and the average duration', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(RUNS)))
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('Import jobs')).toBeInTheDocument()
    expect(screen.getByText(/Average import duration:/)).toBeInTheDocument()
    expect(screen.getByText('30m 30s')).toBeInTheDocument()

    // Column headers.
    for (const header of [
      'Start time',
      'Duration',
      'Successful',
      'Warnings',
      'Errors',
      'New measurements',
      'New stations',
    ]) {
      expect(screen.getByRole('columnheader', { name: header })).toBeInTheDocument()
    }

    // Row values.
    expect(screen.getByText('1h 0m')).toBeInTheDocument()
    expect(screen.getAllByText('Yes').length).toBeGreaterThan(0)
    expect(screen.getAllByText('No').length).toBeGreaterThan(0)
    expect(screen.getByText('120')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
  })

  it('shows the empty state when there are no runs yet', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(ok({ items: [], average_duration_seconds: null })),
    )
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('No import jobs recorded yet.')).toBeInTheDocument()
    expect(screen.queryByText(/Average import duration:/)).not.toBeInTheDocument()
  })

  it('shows an error message when the history request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))
    render(<ImportHistory dataSourceId="ms" />)

    expect(await screen.findByText('Could not load the import jobs.')).toBeInTheDocument()
  })
})
