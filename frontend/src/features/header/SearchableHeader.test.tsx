import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { MemoryRouter } from 'react-router-dom'
import { SearchableHeader } from './SearchableHeader'
import { TrendSettingsProvider } from '../settings/TrendSettingsContext'
import type { StationSummary } from '../stations/types'

const GLOBAL_SUMMARY = {
  station_count: 4,
  channel_count: 8,
  bikes_last_day_total: 120,
  last_update: null,
}

const STATION: StationSummary = {
  id: 'zoo',
  name: 'Zoo Station',
  description: 'Near the zoo entrance',
  latitude: 51.96,
  longitude: 7.63,
  channel_count: 2,
  bikes_last_day: 100,
  image_url: '/img/zoo.png',
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function searchPayload() {
  return {
    items: [STATION],
    actions: {
      find_on_map: { enabled: true },
      open_detail: { enabled: true },
    },
  }
}

// Serves the header's global-summary request and the search dialog's station
// request from one fake fetch.
function stubFetch() {
  const fetchMock = vi.fn((input: unknown) => {
    const url = String(input)
    if (url.includes('/global-summary')) return Promise.resolve(ok(GLOBAL_SUMMARY))
    return Promise.resolve(ok(searchPayload()))
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function renderHeader() {
  const onSelect = vi.fn()
  const onFind = vi.fn()
  const onDetail = vi.fn()
  render(
    <MemoryRouter>
      <TrendSettingsProvider>
        <SearchableHeader onSelect={onSelect} onFind={onFind} onDetail={onDetail} />
      </TrendSettingsProvider>
    </MemoryRouter>,
  )
  return { onSelect, onFind, onDetail }
}

function openSearchDialog() {
  const callbacks = renderHeader()
  fireEvent.click(screen.getByRole('button', { name: 'Search counting stations…' }))
  return callbacks
}

beforeEach(() => {
  localStorage.clear()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('SearchableHeader', () => {
  it('renders the top bar without the search dialog initially', async () => {
    stubFetch()
    renderHeader()

    expect(screen.getByRole('link', { name: 'Bike Counter' })).toBeInTheDocument()
    expect(
      screen.queryByPlaceholderText(/Filter stations by name or description/),
    ).not.toBeInTheDocument()

    // Let the global-summary request settle so nothing updates after the test.
    await screen.findByTitle('Show global summary')
  })

  it('opens the search dialog from the top bar', async () => {
    stubFetch()
    openSearchDialog()

    const input = await screen.findByPlaceholderText(/Filter stations by name or description/)
    expect(input).toBeInTheDocument()
  })

  it('calls onSelect with the station and closes the dialog', async () => {
    stubFetch()
    const { onSelect } = openSearchDialog()

    fireEvent.click(await screen.findByRole('button', { name: /Zoo Station/ }))
    expect(onSelect).toHaveBeenCalledTimes(1)
    expect(onSelect).toHaveBeenCalledWith(STATION)
    expect(
      screen.queryByPlaceholderText(/Filter stations by name or description/),
    ).not.toBeInTheDocument()
  })

  it('calls onFind and closes the dialog', async () => {
    stubFetch()
    const { onFind } = openSearchDialog()

    fireEvent.click(await screen.findByRole('button', { name: 'Find on map' }))
    expect(onFind).toHaveBeenCalledTimes(1)
    expect(onFind).toHaveBeenCalledWith(STATION)
    expect(
      screen.queryByPlaceholderText(/Filter stations by name or description/),
    ).not.toBeInTheDocument()
  })

  it('calls onDetail and closes the dialog', async () => {
    stubFetch()
    const { onDetail } = openSearchDialog()

    fireEvent.click(await screen.findByRole('button', { name: 'Open detail' }))
    expect(onDetail).toHaveBeenCalledTimes(1)
    expect(onDetail).toHaveBeenCalledWith(STATION)
    expect(
      screen.queryByPlaceholderText(/Filter stations by name or description/),
    ).not.toBeInTheDocument()
  })

  it('closes the dialog with the Escape key when it is open', async () => {
    stubFetch()
    openSearchDialog()

    expect(
      await screen.findByPlaceholderText(/Filter stations by name or description/),
    ).toBeInTheDocument()

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(
      screen.queryByPlaceholderText(/Filter stations by name or description/),
    ).not.toBeInTheDocument()
  })

  it('closes the dialog via its Close button', async () => {
    stubFetch()
    openSearchDialog()

    fireEvent.click(await screen.findByRole('button', { name: 'Close' }))
    expect(
      screen.queryByPlaceholderText(/Filter stations by name or description/),
    ).not.toBeInTheDocument()
  })
})
