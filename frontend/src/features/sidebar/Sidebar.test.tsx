import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { SidebarShell, SidebarStation, SidebarStationStats } from '../stations/types'
import { Sidebar } from './Sidebar'

const STATS_URL = '/api/bff/stations/sidebar/stats?bounds'

function station(id: string, name: string, extra: Partial<SidebarStation> = {}): SidebarStation {
  return {
    id,
    name,
    description: `${name} description`,
    latitude: 51.9,
    longitude: 7.6,
    image_url: `/img/${id}.png`,
    ...extra,
  }
}

function shell(items: SidebarStation[], visible = items.length): SidebarShell {
  return { items, visible_count: visible, total_count: 5, _links: { stats: STATS_URL } }
}

function statsOf(entries: Array<[string, number]>): Map<string, SidebarStationStats> {
  return new Map(
    entries.map(([id, bikes]) => [id, { station_id: id, channel_count: 1, bikes_last_day: bikes }]),
  )
}

function renderSidebar(
  overrides: {
    shell?: SidebarShell | null
    stats?: Map<string, SidebarStationStats> | null
    loading?: boolean
    error?: boolean
    statsError?: boolean
    onSelectStation?: (station: SidebarStation) => void
    onSummarize?: () => void
    onClose?: () => void
  } = {},
) {
  const {
    shell: shellValue = shell([]),
    stats = null,
    loading = false,
    error = false,
    statsError = false,
    onSelectStation = vi.fn(),
    onSummarize = vi.fn(),
    onClose,
  } = overrides
  const utils = render(
    <Sidebar
      shell={shellValue}
      stats={stats}
      loading={loading}
      error={error}
      statsError={statsError}
      onSelectStation={onSelectStation}
      onSummarize={onSummarize}
      onClose={onClose}
    />,
  )
  return { ...utils, onSelectStation, onSummarize, onClose }
}

describe('Sidebar', () => {
  it('renders the header and the visible/total badge once the shell is present', () => {
    renderSidebar({ shell: shell([station('a', 'Alpha'), station('b', 'Beta')]) })

    expect(screen.getByText('Visible counting stations')).toBeInTheDocument()
    expect(screen.getByText('2 / 5')).toBeInTheDocument()
  })

  it('shows a placeholder badge while the shell is still missing', () => {
    renderSidebar({ shell: null, loading: true })

    expect(screen.getByText('–')).toBeInTheDocument()
  })

  it('sorts the visible stations by their last-day bike count, then by name', () => {
    const items = [station('a', 'Alpha'), station('b', 'Beta'), station('c', 'Zeta')]
    renderSidebar({
      shell: shell(items),
      stats: statsOf([
        ['a', 30],
        ['b', 30],
        ['c', 100],
      ]),
    })

    const order = within(screen.getByRole('list'))
      .getAllByRole('listitem')
      .map((item) => item.textContent ?? '')
    expect(order[0]).toContain('Zeta')
    expect(order[1]).toContain('Alpha')
    expect(order[2]).toContain('Beta')
  })

  it('keeps the shell order while the stats are still loading', () => {
    const items = [station('a', 'Alpha'), station('b', 'Beta')]
    renderSidebar({ shell: shell(items), stats: null })

    const order = within(screen.getByRole('list'))
      .getAllByRole('listitem')
      .map((item) => item.textContent ?? '')
    expect(order[0]).toContain('Alpha')
    expect(order[1]).toContain('Beta')
  })

  it('renders skeleton rows while loading and no shell is available yet', () => {
    const { container } = renderSidebar({ shell: null, loading: true })

    const list = screen.getByRole('list')
    expect(list).toHaveAttribute('aria-busy', 'true')
    expect(within(list).getAllByRole('listitem')).toHaveLength(5)
    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0)
  })

  it('shows the empty state and disables summarizing when no station is visible', () => {
    renderSidebar({ shell: shell([]), loading: false })

    expect(screen.getByText('No counting stations visible in this area.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Summarize visible stations/ })).toBeDisabled()
  })

  it('shows the error text and no list when the shell fetch failed', () => {
    const { onSummarize } = renderSidebar({ shell: null, loading: false, error: true })

    expect(screen.getByText('Could not load counting stations.')).toBeInTheDocument()
    expect(screen.queryByRole('list')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Summarize visible stations/ })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: /Summarize visible stations/ }))
    expect(onSummarize).not.toHaveBeenCalled()
  })

  it('announces the stats loading state via an sr-only paragraph', () => {
    renderSidebar({
      shell: shell([station('a', 'Alpha')]),
      stats: null,
      loading: false,
    })

    expect(screen.getByText('Loading station statistics…')).toBeInTheDocument()
  })

  it('shows the stats error text and hides the loading announcement when stats failed', () => {
    renderSidebar({
      shell: shell([station('a', 'Alpha')]),
      stats: null,
      loading: false,
      statsError: true,
    })

    expect(screen.getByText('Could not load station statistics.')).toBeInTheDocument()
    expect(screen.queryByText('Loading station statistics…')).not.toBeInTheDocument()
  })

  it('enables summarizing and fires onSummarize when stations are listed', () => {
    const { onSummarize } = renderSidebar({
      shell: shell([station('a', 'Alpha')]),
      stats: null,
      loading: false,
    })

    const summarize = screen.getByRole('button', { name: /Summarize visible stations/ })
    expect(summarize).toBeEnabled()
    fireEvent.click(summarize)
    expect(onSummarize).toHaveBeenCalledTimes(1)
  })

  it('renders a close button only when onClose is provided and fires it', () => {
    const { onClose } = renderSidebar({
      shell: shell([station('a', 'Alpha')]),
      onClose: vi.fn(),
    })

    const closeButton = screen.getByRole('button', { name: 'Close station list' })
    fireEvent.click(closeButton)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('omits the close button when no onClose handler is given', () => {
    renderSidebar({ shell: shell([station('a', 'Alpha')]) })

    expect(screen.queryByRole('button', { name: 'Close station list' })).not.toBeInTheDocument()
  })

  it('forwards station selection through onSelectStation', () => {
    const alpha = station('a', 'Alpha')
    const { onSelectStation } = renderSidebar({
      shell: shell([alpha]),
      stats: statsOf([['a', 10]]),
    })

    fireEvent.click(screen.getByRole('button', { name: /Alpha/ }))
    expect(onSelectStation).toHaveBeenCalledTimes(1)
    expect(onSelectStation).toHaveBeenCalledWith(alpha)
  })
})
