import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { OverviewPanelSkeleton } from './Skeletons'

describe('OverviewPanelSkeleton', () => {
  it('renders the all-time skeleton and four metric-box skeletons', () => {
    const { container } = render(<OverviewPanelSkeleton />)

    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0)
    expect(container.querySelectorAll('li')).toHaveLength(4)
  })
})
