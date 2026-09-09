import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { GlobalSummaryDialog } from './GlobalSummaryDialog'
import type { GlobalSummary } from './types'

const SUMMARY: GlobalSummary = {
  station_count: 5,
  channel_count: 3,
  bikes_last_day_total: 42,
  last_update: null,
}

describe('GlobalSummaryDialog', () => {
  it('renders the title and the four facts from the summary', () => {
    render(<GlobalSummaryDialog summary={SUMMARY} onClose={vi.fn()} />)

    expect(screen.getByRole('heading', { name: 'Global summary' })).toBeInTheDocument()
    expect(screen.getByText('Whole-system counting statistics.')).toBeInTheDocument()

    expect(screen.getByText('Counting stations')).toBeInTheDocument()
    expect(screen.getByText('5')).toBeInTheDocument()
    expect(screen.getByText('Channels')).toBeInTheDocument()
    expect(screen.getByText('3')).toBeInTheDocument()
    expect(screen.getByText('Bikes / last day')).toBeInTheDocument()
    expect(screen.getByText('42')).toBeInTheDocument()
    expect(screen.getByText('Updated')).toBeInTheDocument()
    // A null last_update renders as "never".
    expect(screen.getByText('never')).toBeInTheDocument()
  })

  it('formats a present last_update timestamp instead of "never"', () => {
    render(
      <GlobalSummaryDialog
        summary={{ ...SUMMARY, last_update: '2026-01-02T12:00:00Z' }}
        onClose={vi.fn()}
      />,
    )

    expect(screen.getByText('Updated')).toBeInTheDocument()
    expect(screen.queryByText('never')).not.toBeInTheDocument()
    expect(screen.getByText(/^\d{2}\.\d{2}/)).toBeInTheDocument()
  })

  it('invokes onClose when the dialog is dismissed', () => {
    const onClose = vi.fn()
    render(
      <GlobalSummaryDialog
        summary={{ ...SUMMARY, last_update: '2026-01-02T12:00:00Z' }}
        onClose={onClose}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
