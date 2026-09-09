import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { MemoryRouter } from 'react-router-dom'
import { TopBar } from './TopBar'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import type { GlobalSummary } from './types'

const SUMMARY: GlobalSummary = {
  station_count: 4,
  channel_count: 8,
  bikes_last_day_total: 120,
  last_update: '2026-01-02T12:00:00Z',
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function renderTopBar(fetchImpl: () => Promise<Response> | Response) {
  const fetchMock = vi.fn(fetchImpl)
  vi.stubGlobal('fetch', fetchMock)
  const onOpenSearch = vi.fn()
  const utils = render(
    <MemoryRouter>
      <TrendSettingsProvider>
        <TopBar onOpenSearch={onOpenSearch} />
      </TrendSettingsProvider>
    </MemoryRouter>,
  )
  return { ...utils, onOpenSearch, fetchMock }
}

beforeEach(() => {
  localStorage.clear()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('TopBar', () => {
  it('renders the brand link home and the data-sources link', async () => {
    renderTopBar(() => ok(SUMMARY))

    expect(screen.getByRole('link', { name: 'Bike Counter' })).toHaveAttribute('href', '/')
    expect(screen.getByRole('link', { name: 'Data sources' })).toHaveAttribute(
      'href',
      '/data-sources',
    )
    // Wait for the summary so no state update happens after the test ends.
    await screen.findByTitle('Show global summary')
  })

  it('calls onOpenSearch from both the desktop and the mobile search trigger', async () => {
    const { onOpenSearch } = renderTopBar(() => ok(SUMMARY))
    await screen.findByTitle('Show global summary')

    fireEvent.click(screen.getByRole('button', { name: 'Search counting stations…' }))
    fireEvent.click(screen.getByRole('button', { name: 'Search counting stations' }))

    expect(onOpenSearch).toHaveBeenCalledTimes(2)
  })

  it('opens the summary dialog when the timestamp button is clicked', async () => {
    renderTopBar(() => ok(SUMMARY))

    const timestampButton = await screen.findByTitle('Show global summary')
    expect(timestampButton.textContent).toMatch(/^updated /)

    fireEvent.click(timestampButton)
    expect(screen.getByRole('heading', { name: 'Global summary' })).toBeInTheDocument()
    expect(screen.getByText('Bikes / last day')).toBeInTheDocument()
  })

  it('shows the unavailable text when the summary request fails', async () => {
    renderTopBar(() => Promise.reject(new Error('network down')))

    expect(await screen.findByText('Global summary unavailable.')).toBeInTheDocument()
  })

  it('shows a skeleton while the summary is loading', () => {
    const { container } = renderTopBar(() => new Promise<Response>(() => {}))

    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull()
    expect(screen.queryByText('Global summary unavailable.')).not.toBeInTheDocument()
  })

  it('only opens the dialog for a loaded summary', async () => {
    const { onOpenSearch } = renderTopBar(() => ok(SUMMARY))
    await screen.findByTitle('Show global summary')

    // The header must not crash when only the search is triggered.
    fireEvent.click(screen.getByRole('button', { name: 'Search counting stations…' }))
    expect(onOpenSearch).toHaveBeenCalledTimes(1)

    await waitFor(() => expect(screen.queryByTitle('Show global summary')).not.toBeNull())
  })
})
