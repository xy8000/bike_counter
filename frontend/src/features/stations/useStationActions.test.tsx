import { act, render, screen } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { describe, expect, it } from 'vitest'
import { useStationActions } from './useStationActions'
import type { StationSummary } from './types'

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

const STATION_WITHOUT_COORDS: StationSummary = {
  ...STATION,
  id: 'alpha',
  latitude: null,
  longitude: null,
}

/// The active route, exposed for assertions (the actions navigate in place).
function LocationProbe() {
  const location = useLocation()
  return <div data-testid="location">{`${location.pathname}${location.search}`}</div>
}

/// Renders the hook inside a router whose location is observable, capturing the
/// hook's return value so the tests can invoke the navigation actions.
function renderHarness() {
  const captured: { current: ReturnType<typeof useStationActions> | null } = { current: null }

  function Harness() {
    captured.current = useStationActions()
    return <LocationProbe />
  }

  render(
    <MemoryRouter initialEntries={['/summary']}>
      <Harness />
    </MemoryRouter>,
  )
  return captured
}

describe('useStationActions', () => {
  it('opens the station detail route in the same tab', () => {
    const captured = renderHarness()

    act(() => captured.current?.openDetail(STATION))

    expect(screen.getByTestId('location')).toHaveTextContent('/stations/zoo')
  })

  it('returns to the map centred on a station that has coordinates', () => {
    const captured = renderHarness()

    act(() => captured.current?.findOnMap(STATION))

    expect(screen.getByTestId('location')).toHaveTextContent(
      '/?min_lat=51.956&min_lng=7.626&max_lat=51.964&max_lng=7.634&station=zoo',
    )
  })

  it('returns to the map at the station id when there are no coordinates', () => {
    const captured = renderHarness()

    act(() => captured.current?.findOnMap(STATION_WITHOUT_COORDS))

    expect(screen.getByTestId('location')).toHaveTextContent('/?station=alpha')
  })
})
