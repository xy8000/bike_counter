import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TrendIcon } from './TrendIcon'

describe('TrendIcon', () => {
  it.each(['up', 'down', 'flat'] as const)('renders an icon labelled "%s"', (trend) => {
    const { container } = render(<TrendIcon trend={trend} />)
    expect(container.querySelector(`svg[aria-label="${trend}"]`)).not.toBeNull()
  })
})
