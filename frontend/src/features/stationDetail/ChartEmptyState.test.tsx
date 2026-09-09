import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { ChartEmptyState } from './ChartEmptyState'

describe('ChartEmptyState', () => {
  it('renders the default message', () => {
    render(<ChartEmptyState />)
    expect(screen.getByText('No data for this period.')).toBeInTheDocument()
  })

  it('renders a custom message and forwards the className', () => {
    const { container } = render(<ChartEmptyState message="Nothing here." className="py-16" />)
    expect(screen.getByText('Nothing here.')).toBeInTheDocument()
    expect(container.firstChild).toHaveClass('flex', 'items-center', 'justify-center', 'py-16')
  })
})
