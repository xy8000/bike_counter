import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { StationListItem } from './StationListItem'
import type { StationSummary } from './types'

const STATION: StationSummary = {
  id: '1',
  name: 'Alpha Platz',
  description: 'Central transport hub',
  latitude: 51.96,
  longitude: 7.63,
  channel_count: 3,
  bikes_last_day: 1234,
  image_url: '/img/alpha.png',
}

function renderItem(
  overrides: {
    station?: StationSummary
    onSelect?: (station: StationSummary) => void
    onFind?: (station: StationSummary) => void
    onDetail?: (station: StationSummary) => void
    showFind?: boolean
    showDetail?: boolean
  } = {},
) {
  const {
    station = STATION,
    onSelect = vi.fn(),
    onFind,
    onDetail,
    showFind = false,
    showDetail = false,
  } = overrides
  const utils = render(
    <ul>
      <StationListItem
        station={station}
        onSelect={onSelect}
        onFind={onFind}
        onDetail={onDetail}
        showFind={showFind}
        showDetail={showDetail}
      />
    </ul>,
  )
  return { ...utils, onSelect, onFind, onDetail }
}

describe('StationListItem', () => {
  it('renders the identity, channel count and formatted bike count', () => {
    const { container } = renderItem()

    expect(screen.getByText('Alpha Platz')).toBeInTheDocument()
    expect(screen.getByText('Central transport hub')).toBeInTheDocument()
    expect(screen.getByText('3 channels')).toBeInTheDocument()
    // de-DE formatting turns 1234 into "1.234"
    expect(screen.getByText('1.234')).toBeInTheDocument()
    expect(screen.getByText(/bikes \/ last day/)).toBeInTheDocument()

    const image = container.querySelector('img')
    expect(image).toHaveAttribute('src', '/img/alpha.png')
    expect(image).toHaveAttribute('alt', '')
  })

  it('does not render find/detail buttons when neither is requested', () => {
    renderItem()

    expect(screen.queryByRole('button', { name: 'Find on map' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open detail' })).not.toBeInTheDocument()
  })

  it('renders the find button only when shown, wired and the station has coordinates', () => {
    const { onFind } = renderItem({ showFind: true, onFind: vi.fn() })

    const findButton = screen.getByRole('button', { name: 'Find on map' })
    expect(findButton).toBeInTheDocument()
    fireEvent.click(findButton)
    expect(onFind).toHaveBeenCalledWith(STATION)
  })

  it('omits the find button for a station without coordinates', () => {
    renderItem({
      station: { ...STATION, latitude: null, longitude: null },
      showFind: true,
      onFind: vi.fn(),
    })

    expect(screen.queryByRole('button', { name: 'Find on map' })).not.toBeInTheDocument()
  })

  it('omits the find button when showFind is set but no onFind handler is given', () => {
    renderItem({ showFind: true, onFind: undefined })

    expect(screen.queryByRole('button', { name: 'Find on map' })).not.toBeInTheDocument()
  })

  it('omits the find button when showFind is false', () => {
    renderItem({ showFind: false, onFind: vi.fn() })

    expect(screen.queryByRole('button', { name: 'Find on map' })).not.toBeInTheDocument()
  })

  it('renders the detail button when shown and wired, and fires on click', () => {
    const { onDetail } = renderItem({ showDetail: true, onDetail: vi.fn() })

    const detailButton = screen.getByRole('button', { name: 'Open detail' })
    expect(detailButton).toBeInTheDocument()
    fireEvent.click(detailButton)
    expect(onDetail).toHaveBeenCalledWith(STATION)
  })

  it('omits the detail button when showDetail is set but no onDetail handler is given', () => {
    renderItem({ showDetail: true, onDetail: undefined })

    expect(screen.queryByRole('button', { name: 'Open detail' })).not.toBeInTheDocument()
  })

  it('omits the detail button when showDetail is false', () => {
    renderItem({ showDetail: false, onDetail: vi.fn() })

    expect(screen.queryByRole('button', { name: 'Open detail' })).not.toBeInTheDocument()
  })

  it('shows both action buttons together when both are enabled and wired', () => {
    renderItem({ showFind: true, showDetail: true, onFind: vi.fn(), onDetail: vi.fn() })

    expect(screen.getByRole('button', { name: 'Find on map' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open detail' })).toBeInTheDocument()
  })

  it('calls onSelect with the station when the row is clicked', () => {
    const { onSelect } = renderItem()

    fireEvent.click(screen.getByRole('button', { name: /Alpha Platz/ }))
    expect(onSelect).toHaveBeenCalledWith(STATION)
  })
})
