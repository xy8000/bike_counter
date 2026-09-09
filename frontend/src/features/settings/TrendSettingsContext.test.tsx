import { act, render } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import { TrendSettingsProvider, useTrendSettings } from './TrendSettingsContext'

const STORAGE_KEY = 'bike-counter.trends.exclude_new_stations'

interface TrendSettingsValue {
  excludeNewStations: boolean
  setExcludeNewStations: (value: boolean) => void
}

function Probe({ captured }: { captured: { current: TrendSettingsValue | null } }) {
  const value = useTrendSettings()
  captured.current = value
  return null
}

function renderProvider() {
  const captured: { current: TrendSettingsValue | null } = { current: null }
  render(
    <TrendSettingsProvider>
      <Probe captured={captured} />
    </TrendSettingsProvider>,
  )
  const value = () => {
    if (!captured.current) throw new Error('provider probe not initialised')
    return captured.current
  }
  return { value }
}

beforeEach(() => {
  localStorage.clear()
})

describe('TrendSettingsContext', () => {
  it('throws when used outside of its provider', () => {
    expect(() => render(<Probe captured={{ current: null }} />)).toThrow(
      'useTrendSettings must be used within a TrendSettingsProvider',
    )
  })

  it('defaults to false and persists a change to localStorage', () => {
    const { value } = renderProvider()

    expect(value().excludeNewStations).toBe(false)
    // The provider writes the initial value right after mount.
    expect(localStorage.getItem(STORAGE_KEY)).toBe('false')

    act(() => value().setExcludeNewStations(true))
    expect(value().excludeNewStations).toBe(true)
    expect(localStorage.getItem(STORAGE_KEY)).toBe('true')
  })

  it('reads an existing stored value on mount', () => {
    localStorage.setItem(STORAGE_KEY, 'true')

    const { value } = renderProvider()
    expect(value().excludeNewStations).toBe(true)
  })

  it('keeps a non-true stored value as false', () => {
    localStorage.setItem(STORAGE_KEY, 'yes')

    const { value } = renderProvider()
    expect(value().excludeNewStations).toBe(false)
  })
})
