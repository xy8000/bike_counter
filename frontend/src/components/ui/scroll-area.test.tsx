import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { ScrollArea } from './scroll-area'

describe('ScrollArea', () => {
  it('renders a root with data-slot="scroll-area" and the given className', () => {
    const { container } = render(
      <ScrollArea className="h-40 w-64">
        <p>content</p>
      </ScrollArea>,
    )
    const root = container.querySelector<HTMLElement>('[data-slot="scroll-area"]')
    expect(root).not.toBeNull()
    expect(root).toHaveClass('relative')
    expect(root).toHaveClass('h-40', 'w-64')
  })

  it('renders its children inside the viewport', () => {
    const { container } = render(
      <ScrollArea className="h-40">
        <ul>
          <li>first row</li>
          <li>second row</li>
        </ul>
      </ScrollArea>,
    )
    expect(screen.getByText('first row')).toBeInTheDocument()
    expect(screen.getByText('second row')).toBeInTheDocument()
    const viewport = container.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]')
    expect(viewport).not.toBeNull()
    expect(
      screen.getByText('first row').closest('[data-slot="scroll-area-viewport"]'),
    ).not.toBeNull()
  })
})
