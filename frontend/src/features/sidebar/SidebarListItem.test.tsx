import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { SidebarStation, SidebarStationStats } from '../stations/types'
import { SidebarListItem } from './SidebarListItem'

const STATION: SidebarStation = {
  id: '1',
  name: 'Alpha Platz',
  description: 'Central transport hub',
  latitude: 51.96,
  longitude: 7.63,
  image_url: '/img/alpha.png',
}

function renderItem(
  overrides: {
    station?: SidebarStation
    stats?: SidebarStationStats
    onSelect?: (station: SidebarStation) => void
  } = {},
) {
  const { station = STATION, stats, onSelect = vi.fn() } = overrides
  const utils = render(
    <ul>
      <SidebarListItem station={station} stats={stats} onSelect={onSelect} />
    </ul>,
  )
  return { ...utils, onSelect }
}

describe('SidebarListItem', () => {
  it('renders the station identity', () => {
    const { container } = renderItem()

    expect(screen.getByText('Alpha Platz')).toBeInTheDocument()
    expect(screen.getByText('Central transport hub')).toBeInTheDocument()

    const image = container.querySelector('img')
    expect(image).toHaveAttribute('src', '/img/alpha.png')
    expect(image).toHaveAttribute('alt', '')
  })

  it('renders the pluralized channel count and formatted bike count when stats are present', () => {
    renderItem({ stats: { station_id: '1', channel_count: 2, bikes_last_day: 1500 } })

    expect(screen.getByText('2 channels')).toBeInTheDocument()
    // de-DE formatting turns 1500 into "1.500"
    expect(screen.getByText('1.500')).toBeInTheDocument()
    expect(screen.getByText(/bikes \/ last day/)).toBeInTheDocument()
  })

  it('keeps the channel label singular for a single channel', () => {
    renderItem({ stats: { station_id: '1', channel_count: 1, bikes_last_day: 7 } })

    expect(screen.getByText('1 channel')).toBeInTheDocument()
    expect(screen.queryByText('1 channels')).not.toBeInTheDocument()
  })

  it('renders stat skeletons instead of the stats line when stats are missing', () => {
    const { container } = renderItem({ stats: undefined })

    expect(container.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(2)
    expect(screen.queryByText(/channel/)).not.toBeInTheDocument()
  })

  it('calls onSelect with the station when the row is clicked', () => {
    const { onSelect } = renderItem()

    fireEvent.click(screen.getByRole('button', { name: /Alpha Platz/ }))
    expect(onSelect).toHaveBeenCalledTimes(1)
    expect(onSelect).toHaveBeenCalledWith(STATION)
  })
})
