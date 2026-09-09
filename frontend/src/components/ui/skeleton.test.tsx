import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Skeleton } from './skeleton'

describe('Skeleton', () => {
  it('renders a div with data-slot="skeleton" and the pulse base classes', () => {
    const { container } = render(<Skeleton className="h-4 w-16" />)
    const skeleton = container.querySelector<HTMLElement>('[data-slot="skeleton"]')
    expect(skeleton).not.toBeNull()
    expect(skeleton).toHaveClass('animate-pulse')
    expect(skeleton).toHaveClass('h-4', 'w-16')
  })

  it('renders children passed through the div', () => {
    const { getByText } = render(
      <Skeleton>
        <span>loading</span>
      </Skeleton>,
    )
    expect(getByText('loading')).toBeInTheDocument()
  })
})
