import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TrendSettingsIllustration } from './TrendSettingsIllustration'

describe('TrendSettingsIllustration', () => {
  it('renders the all-stations variant with its legend and jump description', () => {
    const { container } = render(<TrendSettingsIllustration excludeNewStations={false} />)

    expect(screen.getByText('All stations')).toBeInTheDocument()
    expect(screen.getByText('existing stations')).toBeInTheDocument()
    expect(screen.getByText('new station')).toBeInTheDocument()
    expect(
      screen.getByText('a station opens partway through the period — totals jump'),
    ).toBeInTheDocument()

    // Month labels from the first and last months (Jan/Aug).
    expect(screen.getByText('J')).toBeInTheDocument()
    expect(screen.getByText('A')).toBeInTheDocument()

    // The decorative block is hidden from the accessibility tree.
    expect(container.querySelector('[aria-hidden="true"]')).not.toBeNull()
    // New-station months get a stacked second bar segment.
    expect(container.querySelectorAll('div[class*="rounded-b-sm"]').length).toBeGreaterThan(0)
  })

  it('renders the established-only variant when new stations are excluded', () => {
    render(<TrendSettingsIllustration excludeNewStations={true} />)

    expect(screen.getByText('Established only')).toBeInTheDocument()
    expect(screen.getByText('only stations open since before the period')).toBeInTheDocument()
    expect(screen.getByText('new station excluded — totals grow more evenly')).toBeInTheDocument()

    // The stacked "new station" legend disappears.
    expect(screen.queryByText('new station')).not.toBeInTheDocument()
  })
})
