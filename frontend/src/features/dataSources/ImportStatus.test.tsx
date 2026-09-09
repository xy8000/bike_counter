import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { FeatureBadges, ImportStatus, RunningDuration, formatSeconds } from './ImportStatus'
import type { DataSourceDetail } from './types'

function fullDetail(overrides: Partial<DataSourceDetail>): DataSourceDetail {
  return {
    id: 'ds1',
    name: 'City',
    provider_type: 'radvis',
    image_url: '',
    station_count: 1,
    channel_count: 2,
    stations: [],
    last_updated_at: null,
    imported_until: null,
    first_data_at: null,
    last_data_at: null,
    has_historical: true,
    has_real_time: true,
    has_full_current_year: true,
    last_import: null,
    ...overrides,
  }
}

afterEach(() => {
  vi.useRealTimers()
})

describe('formatSeconds', () => {
  it('formats whole seconds into a compact duration', () => {
    expect(formatSeconds(0)).toBe('0s')
    expect(formatSeconds(59)).toBe('59s')
    expect(formatSeconds(60)).toBe('1m 0s')
    expect(formatSeconds(61)).toBe('1m 1s')
    expect(formatSeconds(3599)).toBe('59m 59s')
    expect(formatSeconds(3600)).toBe('1h 0m')
    expect(formatSeconds(3725)).toBe('1h 2m')
  })

  it('clamps negative and fractional values to whole seconds', () => {
    expect(formatSeconds(-5)).toBe('0s')
    expect(formatSeconds(5.9)).toBe('5s')
  })
})

describe('RunningDuration', () => {
  it('renders the elapsed time and ticks every second', () => {
    vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] })
    const startedAt = new Date(Date.now() - 65_000).toISOString()
    render(<RunningDuration startedAt={startedAt} />)

    expect(screen.getByText('1m 5s')).toBeInTheDocument()

    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(screen.getByText('1m 6s')).toBeInTheDocument()

    act(() => {
      vi.advanceTimersByTime(59_000)
    })
    expect(screen.getByText('2m 5s')).toBeInTheDocument()
  })
})

describe('ImportStatus', () => {
  it('shows "Never imported" when there is no import run', () => {
    render(<ImportStatus import={null} />)
    expect(screen.getByText('Never imported')).toBeInTheDocument()
  })

  it('shows a running badge with the elapsed duration', () => {
    const startedAt = new Date(Date.now() - 10_000).toISOString()
    render(
      <ImportStatus
        import={{
          status: 'RUNNING',
          started_at: startedAt,
          finished_at: null,
          duration_seconds: null,
          failure_message: null,
          warning_count: 0,
          error_count: 0,
        }}
      />,
    )
    expect(screen.getByText(/Running/)).toBeInTheDocument()
    expect(screen.getByText('10s')).toBeInTheDocument()
  })

  it('shows a failed badge', () => {
    render(
      <ImportStatus
        import={{
          status: 'FAILED',
          started_at: '2026-01-01T10:00:00Z',
          finished_at: '2026-01-01T10:01:00Z',
          duration_seconds: 60,
          failure_message: 'boom',
          warning_count: 0,
          error_count: 0,
        }}
      />,
    )
    expect(screen.getByText('Failed')).toBeInTheDocument()
  })

  it('shows a succeeded badge', () => {
    render(
      <ImportStatus
        import={{
          status: 'FINISHED',
          started_at: '2026-01-01T10:00:00Z',
          finished_at: '2026-01-01T10:00:30Z',
          duration_seconds: 30,
          failure_message: null,
          warning_count: 0,
          error_count: 0,
        }}
      />,
    )
    expect(screen.getByText('Succeeded')).toBeInTheDocument()
  })
})

describe('FeatureBadges', () => {
  it('shows the available features as filled badges', () => {
    render(<FeatureBadges detail={fullDetail({ has_historical: true })} />)
    expect(screen.getByText('Historical data')).toBeInTheDocument()
    expect(screen.getByText('Real-time data')).toBeInTheDocument()
    expect(screen.getByText('Full current year coverage')).toBeInTheDocument()
    expect(screen.queryByText('No historical data')).not.toBeInTheDocument()
  })

  it('marks the missing features as unavailable', () => {
    render(
      <FeatureBadges
        detail={fullDetail({
          has_historical: false,
          has_real_time: false,
          has_full_current_year: false,
        })}
      />,
    )
    expect(screen.getByText('No historical data')).toBeInTheDocument()
    expect(screen.getByText('No real-time data')).toBeInTheDocument()
    expect(screen.getByText('No full current year coverage')).toBeInTheDocument()
  })
})
