import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Label } from './label'

describe('Label', () => {
  it('renders a label with htmlFor and its children', () => {
    render(<Label htmlFor="email">Email</Label>)
    const label = screen.getByText('Email')
    expect(label.tagName).toBe('LABEL')
    expect(label).toHaveAttribute('data-slot', 'label')
    expect(label).toHaveAttribute('for', 'email')
  })

  it('merges an extra className onto the label', () => {
    render(
      <Label htmlFor="x" className="my-extra">
        Name
      </Label>,
    )
    expect(screen.getByText('Name')).toHaveClass('my-extra')
  })

  it('renders without a htmlFor attribute when omitted', () => {
    render(<Label>Bare</Label>)
    expect(screen.getByText('Bare')).not.toHaveAttribute('for')
  })
})
