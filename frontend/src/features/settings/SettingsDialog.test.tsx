import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { SettingsDialog } from './SettingsDialog'
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

function renderDialog(settings: TimeframeSettingsValue) {
  const onOpenChange = vi.fn()
  render(
    <TrendSettingsProvider>
      <SettingsDialog open onOpenChange={onOpenChange} settings={settings} />
    </TrendSettingsProvider>,
  )
  return { onOpenChange }
}

beforeEach(() => {
  localStorage.clear()
})

describe('SettingsDialog', () => {
  it('renders the timeframe options and highlights the active one', () => {
    renderDialog(makeSettings({ timeframe: 'week' }))

    expect(screen.getByRole('heading', { name: 'Bike-Trends settings' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '24 hours' })).toHaveAttribute(
      'aria-pressed',
      'false',
    )
    expect(screen.getByRole('button', { name: 'This week' })).toHaveAttribute(
      'aria-pressed',
      'true',
    )
    expect(screen.getByRole('button', { name: 'Last 30 days' })).toHaveAttribute(
      'aria-pressed',
      'false',
    )
    expect(screen.getByRole('button', { name: 'This year' })).toHaveAttribute(
      'aria-pressed',
      'false',
    )
    expect(screen.getByRole('button', { name: 'Individual' })).toHaveAttribute(
      'aria-pressed',
      'false',
    )
  })

  it('calls setTimeframe when a timeframe option is clicked', () => {
    const settings = makeSettings({ timeframe: 'week' })
    renderDialog(settings)

    fireEvent.click(screen.getByRole('button', { name: 'Last 30 days' }))
    expect(settings.setTimeframe).toHaveBeenCalledWith('last_30_days')
  })

  it('shows the date pickers and disables compare for an individual range', () => {
    const settings = makeSettings({
      timeframe: 'individual',
      isIndividual: true,
      from: '2024-01-01',
      to: '2024-02-01',
    })
    renderDialog(settings)

    const fromInput = screen.getByLabelText('From') as HTMLInputElement
    const toInput = screen.getByLabelText('To') as HTMLInputElement
    expect(fromInput).toHaveValue('2024-01-01')
    expect(toInput).toHaveValue('2024-02-01')

    fireEvent.change(fromInput, { target: { value: '2024-01-15' } })
    expect(settings.setFrom).toHaveBeenCalledWith('2024-01-15')

    fireEvent.change(toInput, { target: { value: '2024-02-15' } })
    expect(settings.setTo).toHaveBeenCalledWith('2024-02-15')

    const compare = screen.getByRole('checkbox', { name: 'Compare previous period' })
    expect(compare).toBeDisabled()
    expect(
      screen.getByText(
        'Comparing to a previous period is not available for an individual date range.',
      ),
    ).toBeInTheDocument()
  })

  it('renders the resolution options and calls setResolution', () => {
    const settings = makeSettings({ timeframe: 'week', resolution: 'mid' })
    renderDialog(settings)

    expect(screen.getByRole('button', { name: '30 Min' })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: 'Hour' })).toHaveAttribute('aria-pressed', 'true')
    const day = screen.getByRole('button', { name: 'Day' })
    expect(day).toHaveAttribute('aria-pressed', 'false')

    fireEvent.click(day)
    expect(settings.setResolution).toHaveBeenCalledWith('low')
  })

  it('toggles the compare checkbox for a fixed timeframe', () => {
    const settings = makeSettings({ timeframe: 'week', compare: false })
    renderDialog(settings)

    const compare = screen.getByRole('checkbox', { name: 'Compare previous period' })
    expect(compare).not.toBeDisabled()
    fireEvent.click(compare)
    expect(settings.setCompare).toHaveBeenCalledWith(true)
  })

  it('keeps the app-global and the shareable exclude flag in sync via the switch', () => {
    const settings = makeSettings({ timeframe: 'week', exclude: false })
    renderDialog(settings)

    const sw = screen.getByRole('switch', { name: /Exclude new stations from trends/ })
    expect(sw).toHaveAttribute('data-state', 'unchecked')

    fireEvent.click(sw)

    expect(settings.setExclude).toHaveBeenCalledWith(true)
    // The context setter ran too, flipping the switch and persisting to storage.
    expect(
      screen.getByRole('switch', { name: /Exclude new stations from trends/ }),
    ).toHaveAttribute('data-state', 'checked')
    expect(localStorage.getItem('bike-counter.trends.exclude_new_stations')).toBe('true')
  })

  it('closes the dialog through the Apply button', () => {
    const { onOpenChange } = renderDialog(makeSettings({ timeframe: 'week' }))

    fireEvent.click(screen.getByRole('button', { name: 'Apply' }))
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it('closes the dialog through its close button', () => {
    const { onOpenChange } = renderDialog(makeSettings({ timeframe: 'week' }))

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })
})
