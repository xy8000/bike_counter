import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import {
  ChartsSkeleton,
  KeyFactsSkeleton,
  MetricBoxSkeleton,
  MonthlyBarSkeleton,
  OverviewSkeleton,
  PageShellSkeleton,
  TotalBikesSkeleton,
} from './Skeletons'

function skeletonCount(container: HTMLElement) {
  return container.querySelectorAll('[data-slot="skeleton"]').length
}

describe('detail page skeletons', () => {
  it('renders the page shell skeleton', () => {
    const { container } = render(<PageShellSkeleton />)
    expect(skeletonCount(container)).toBeGreaterThan(0)
  })

  it('renders the metric box skeleton', () => {
    const { container } = render(<MetricBoxSkeleton />)
    expect(skeletonCount(container)).toBeGreaterThan(0)
  })

  it('renders the total bikes skeleton', () => {
    const { container } = render(<TotalBikesSkeleton />)
    expect(skeletonCount(container)).toBeGreaterThan(0)
  })

  it('renders the overview skeleton with its four metric boxes', () => {
    const { container } = render(<OverviewSkeleton />)
    // One all-time block (1) + four metric boxes (4 × 4 skeletons).
    expect(skeletonCount(container)).toBe(17)
  })

  it('renders the charts skeleton', () => {
    const { container } = render(<ChartsSkeleton />)
    expect(skeletonCount(container)).toBeGreaterThan(0)
  })

  it('renders the key facts skeleton with four boxes', () => {
    const { container } = render(<KeyFactsSkeleton />)
    expect(skeletonCount(container)).toBe(12)
  })

  it('renders the monthly bar skeleton', () => {
    const { container } = render(<MonthlyBarSkeleton />)
    expect(skeletonCount(container)).toBe(7)
  })
})
