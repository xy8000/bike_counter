import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { ChartCard } from './ChartCard'

describe('ChartCard', () => {
  it('renders the title, subtitle, children and note', () => {
    render(
      <ChartCard title="Weekdays" subtitle="the current week" note="Bike-Trends info">
        <p>chart body</p>
      </ChartCard>,
    )

    expect(screen.getByText('Weekdays')).toBeInTheDocument()
    expect(screen.getByText('the current week')).toBeInTheDocument()
    expect(screen.getByText('chart body')).toBeInTheDocument()
    expect(screen.getByText('Bike-Trends info')).toBeInTheDocument()
  })

  it('omits the optional subtitle and note when not provided', () => {
    render(<ChartCard title="Only a title">content</ChartCard>)

    expect(screen.getByText('Only a title')).toBeInTheDocument()
    expect(screen.getByText('content')).toBeInTheDocument()
    expect(screen.queryByText('the current week')).not.toBeInTheDocument()
    expect(screen.queryByText('Bike-Trends info')).not.toBeInTheDocument()
  })
})
