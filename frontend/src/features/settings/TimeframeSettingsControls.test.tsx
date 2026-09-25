import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { TimeframeSettingsControls } from './TimeframeSettingsControls'
import { TrendSettingsProvider } from './TrendSettingsContext'
import type { TimeframeSettingsValue } from './useTimeframeSettings'

function makeSettings(overrides: Partial<TimeframeSettingsValue> = {}): TimeframeSettingsValue {
  return {
    timeframe: 'week',
    from: null,
    to: null,
    compare: false,
    exclude: false,
    resolution: 'mid',
    isIndividual: false,
    setTimeframe: vi.fn(),
    setFrom: vi.fn(),
    setTo: vi.fn(),
    setCompare: vi.fn(),
    setExclude: vi.fn(),
    setResolution: vi.fn(),
    ...overrides,
  }
}

function renderControls(settings: TimeframeSettingsValue) {
  render(
    <TrendSettingsProvider>
      <TimeframeSettingsControls settings={settings} />
    </TrendSettingsProvider>,
  )
}

beforeEach(() => {
  localStorage.clear()
})

describe('TimeframeSettingsControls', () => {
  it('prints the timeframe summary label next to the settings button', () => {
    renderControls(makeSettings({ timeframe: 'week' }))

    expect(screen.getByText('This week')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Calculation settings' })).toBeInTheDocument()
  })

  it('opens the settings dialog through the settings button', () => {
    renderControls(makeSettings({ timeframe: 'week' }))

    expect(screen.queryByText('Bike-Trends settings')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Calculation settings' }))

    expect(screen.getByText('Bike-Trends settings')).toBeInTheDocument()
  })

  it('closes the dialog again through Apply', async () => {
    renderControls(makeSettings({ timeframe: 'week' }))

    fireEvent.click(screen.getByRole('button', { name: 'Calculation settings' }))
    expect(screen.getByText('Bike-Trends settings')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Apply' }))

    await waitFor(() => expect(screen.queryByText('Bike-Trends settings')).not.toBeInTheDocument())
  })
})
