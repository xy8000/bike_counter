import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { MetricCard } from './MetricCard'
import type { StationOverviewMetric } from './types'

const metric: StationOverviewMetric = {
  key: 'last_day',
  current: 12,
  previous: 10,
  trend: 'up',
  delta_percent: 20,
  is_new: false,
}

describe('MetricCard', () => {
  it('renders the label, the current bikes and the positive delta vs the previous period', () => {
    render(<MetricCard metric={metric} />)

    expect(screen.getByText('Last 24 hours')).toBeInTheDocument()
    expect(screen.getByText('12')).toBeInTheDocument()
    expect(screen.getByText('bikes')).toBeInTheDocument()
    expect(screen.getByText('+20%')).toBeInTheDocument()
    expect(screen.getByText('vs. 10')).toBeInTheDocument()
  })

  it('keeps the minus sign for a negative delta and shows a dash for a null delta', () => {
    const { rerender } = render(<MetricCard metric={{ ...metric, delta_percent: -5 }} />)
    expect(screen.getByText('-5%')).toBeInTheDocument()

    rerender(<MetricCard metric={{ ...metric, delta_percent: null, trend: 'flat' }} />)
    expect(screen.getByText('–')).toBeInTheDocument()
    expect(screen.getByText('vs. 10')).toBeInTheDocument()
  })

  it('shows a neutral New indicator instead of a trend when the station is new', () => {
    render(<MetricCard metric={{ ...metric, is_new: true }} />)

    expect(screen.getByText('New')).toBeInTheDocument()
    expect(screen.queryByText('+20%')).not.toBeInTheDocument()
    expect(screen.getByText('vs. 10')).toBeInTheDocument()
  })

  it('labels every known metric key', () => {
    const { rerender } = render(<MetricCard metric={{ ...metric, key: 'last_7_days' }} />)
    expect(screen.getByText('Last 7 days')).toBeInTheDocument()

    rerender(<MetricCard metric={{ ...metric, key: 'last_month' }} />)
    expect(screen.getByText('Last month')).toBeInTheDocument()

    rerender(<MetricCard metric={{ ...metric, key: 'last_year' }} />)
    expect(screen.getByText('Last year')).toBeInTheDocument()
  })
})
