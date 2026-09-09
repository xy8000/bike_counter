import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { TimeframeSettingsLabel } from './TimeframeSettingsLabel'
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

describe('TimeframeSettingsLabel', () => {
  it('formats an individual range as Individual with the localized dates', () => {
    render(
      <TimeframeSettingsLabel
        settings={makeSettings({
          timeframe: 'individual',
          isIndividual: true,
          from: '2024-01-02',
          to: '2024-03-31',
        })}
      />,
    )

    expect(screen.getByText('Individual · 02.01.2024 – 31.03.2024')).toBeInTheDocument()
  })

  it('renders the fixed-timeframe label', () => {
    render(<TimeframeSettingsLabel settings={makeSettings({ timeframe: 'year' })} />)

    expect(screen.getByText('This year')).toBeInTheDocument()
  })

  it('appends the compare suffix when compare is on', () => {
    render(
      <TimeframeSettingsLabel
        settings={makeSettings({ timeframe: 'last_30_days', compare: true })}
      />,
    )

    expect(screen.getByText('Last 30 days · Compare previous period')).toBeInTheDocument()
  })

  it('falls back to a bare Individual label when the range is incomplete', () => {
    render(
      <TimeframeSettingsLabel
        settings={makeSettings({
          timeframe: 'individual',
          isIndividual: true,
          from: null,
          to: null,
        })}
      />,
    )

    expect(screen.getByText('Individual')).toBeInTheDocument()
  })
})
