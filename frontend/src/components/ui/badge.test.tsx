import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Badge, badgeVariants } from './badge'

describe('Badge', () => {
  it('renders children inside a span with data-slot="badge"', () => {
    render(<Badge>Status</Badge>)
    const badge = screen.getByText('Status')
    expect(badge).toHaveAttribute('data-slot', 'badge')
    expect(badge.tagName).toBe('SPAN')
  })

  it.each([
    ['default', 'bg-primary'],
    ['secondary', 'bg-secondary'],
    ['outline', 'text-foreground'],
    ['destructive', 'bg-destructive'],
  ] as const)('applies the %s variant classes', (variant, expectedClass) => {
    render(<Badge variant={variant}>x</Badge>)
    expect(screen.getByText('x')).toHaveClass(expectedClass)
  })

  it('merges an extra className onto the badge', () => {
    render(<Badge className="my-extra">x</Badge>)
    expect(screen.getByText('x')).toHaveClass('my-extra')
  })

  it('renders the child element instead of a span when asChild is set', () => {
    render(
      <Badge asChild>
        <a href="/tag">Link tag</a>
      </Badge>,
    )
    const link = screen.getByRole('link', { name: 'Link tag' })
    expect(link).toHaveAttribute('data-slot', 'badge')
    expect(link.tagName).toBe('A')
    expect(screen.queryByText('Link tag')?.tagName).toBe('A')
  })
})

describe('badgeVariants', () => {
  it('returns the outline classes for the outline variant', () => {
    expect(badgeVariants({ variant: 'outline' })).toContain('text-foreground')
  })
})
