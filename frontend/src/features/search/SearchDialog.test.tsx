import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SearchDialog } from './SearchDialog'
import type { StationSummary } from '../stations/types'

const STATIONS: StationSummary[] = [
  {
    id: 'zoo',
    name: 'Zoo Station',
    description: 'Near the zoo entrance',
    latitude: 51.96,
    longitude: 7.63,
    channel_count: 2,
    bikes_last_day: 100,
    image_url: '/img/zoo.png',
  },
  {
    id: 'alpha',
    name: 'Alpha Platz',
    description: 'Central transport hub',
    latitude: null,
    longitude: null,
    channel_count: 1,
    bikes_last_day: 5,
    image_url: '/img/alpha.png',
  },
]

function searchPayload(items: StationSummary[], actions: Record<string, { enabled: boolean }>) {
  return { items, actions }
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

function renderDialog() {
  const onClose = vi.fn()
  const onSelect = vi.fn()
  const onFind = vi.fn()
  const onDetail = vi.fn()
  render(<SearchDialog onClose={onClose} onSelect={onSelect} onFind={onFind} onDetail={onDetail} />)
  return { onClose, onSelect, onFind, onDetail }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('SearchDialog', () => {
  it('shows the loading state while stations are being fetched', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise(() => {})),
    )
    renderDialog()

    expect(screen.getByText('Loading stations…')).toBeInTheDocument()
  })

  it('shows the error state when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))
    renderDialog()

    expect(await screen.findByText('Could not load stations.')).toBeInTheDocument()
    expect(screen.queryByText('Loading stations…')).not.toBeInTheDocument()
  })

  it('shows the no-results state when no station matches', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(searchPayload([], {}))))
    renderDialog()

    expect(await screen.findByText('No stations match your search.')).toBeInTheDocument()
  })

  it('lists the matching stations alphabetically and shows the enabled actions', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        ok(
          searchPayload(STATIONS, {
            find_on_map: { enabled: true },
            open_detail: { enabled: true },
          }),
        ),
      ),
    )
    renderDialog()

    expect(await screen.findByText('Zoo Station')).toBeInTheDocument()
    expect(screen.getByText('Alpha Platz')).toBeInTheDocument()
    expect(screen.queryByText('Loading stations…')).not.toBeInTheDocument()

    // Alpha has no coordinates, so only the findable Zoo station gets the
    // find-on-map button; both stations get an open-detail button.
    const findButtons = screen.getAllByRole('button', { name: 'Find on map' })
    expect(findButtons).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: 'Open detail' })).toHaveLength(2)
  })

  it('filters the results while typing and clears the filter again', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(ok(searchPayload(STATIONS, { find_on_map: { enabled: true } }))),
    )
    renderDialog()

    const input = await screen.findByPlaceholderText(/Filter stations by name or description/)
    fireEvent.change(input, { target: { value: 'ZOO' } })

    expect(screen.getByText('Zoo Station')).toBeInTheDocument()
    expect(screen.queryByText('Alpha Platz')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Clear filter' }))
    expect(screen.getByText('Alpha Platz')).toBeInTheDocument()
    expect(screen.getByText('Zoo Station')).toBeInTheDocument()
  })

  it('invokes onClose when the Close button is clicked', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(searchPayload([], {}))))
    const { onClose } = renderDialog()

    fireEvent.click(await screen.findByRole('button', { name: 'Close' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('invokes onSelect with the station when its row is clicked', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(searchPayload(STATIONS, {}))))
    const { onSelect } = renderDialog()

    fireEvent.click(await screen.findByRole('button', { name: /Zoo Station/ }))
    expect(onSelect).toHaveBeenCalledTimes(1)
    expect(onSelect).toHaveBeenCalledWith(STATIONS[0])
  })

  it('invokes onFind when a station find-on-map action is used', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(ok(searchPayload(STATIONS, { find_on_map: { enabled: true } }))),
    )
    const { onFind } = renderDialog()

    const findButton = await screen.findByRole('button', { name: 'Find on map' })
    fireEvent.click(findButton)
    expect(onFind).toHaveBeenCalledTimes(1)
    expect(onFind).toHaveBeenCalledWith(STATIONS[0])
  })

  it('does not offer find-on-map when the action is disabled but still opens detail', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(ok(searchPayload(STATIONS, { open_detail: { enabled: true } }))),
    )
    const { onDetail } = renderDialog()

    expect(await screen.findByText('Zoo Station')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Find on map' })).not.toBeInTheDocument()

    const detailButtons = screen.getAllByRole('button', { name: 'Open detail' })
    expect(detailButtons).toHaveLength(2)
    fireEvent.click(detailButtons[0])
    expect(onDetail).toHaveBeenCalledTimes(1)
    expect(onDetail).toHaveBeenCalledWith(STATIONS[1])
  })
})
