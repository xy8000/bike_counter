import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Button, buttonVariants } from './button'

describe('Button', () => {
  it('renders a native button with data-slot="button" and its children', () => {
    render(<Button>Click me</Button>)
    const button = screen.getByRole('button', { name: 'Click me' })
    expect(button).toHaveAttribute('data-slot', 'button')
    expect(button.tagName).toBe('BUTTON')
  })

  it('applies the default variant and size classes', () => {
    render(<Button>Click me</Button>)
    const button = screen.getByRole('button')
    expect(button).toHaveClass('bg-primary')
    expect(button).toHaveClass('h-9')
  })

  it.each([
    ['secondary', 'bg-secondary'],
    ['destructive', 'bg-destructive'],
    ['ghost', 'hover:bg-accent'],
    ['link', 'underline-offset-4'],
    ['outline', 'border'],
  ] as const)('applies the %s variant classes', (variant, expectedClass) => {
    render(<Button variant={variant}>x</Button>)
    expect(screen.getByRole('button')).toHaveClass(expectedClass)
  })

  it.each([
    ['sm', 'h-8'],
    ['lg', 'h-10'],
    ['icon', 'size-9'],
  ] as const)('applies the %s size classes', (size, expectedClass) => {
    render(<Button size={size}>x</Button>)
    expect(screen.getByRole('button')).toHaveClass(expectedClass)
  })

  it('does not set an explicit type attribute when none is given', () => {
    render(<Button>Go</Button>)
    expect(screen.getByRole('button')).not.toHaveAttribute('type')
  })

  it('forwards an explicit type attribute', () => {
    render(<Button type="submit">Go</Button>)
    expect(screen.getByRole('button')).toHaveAttribute('type', 'submit')
  })

  it('fires onClick when clicked', async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    render(<Button onClick={onClick}>Go</Button>)
    await user.click(screen.getByRole('button', { name: 'Go' }))
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('is disabled and does not fire onClick', async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    render(
      <Button disabled onClick={onClick}>
        Go
      </Button>,
    )
    const button = screen.getByRole('button', { name: 'Go' })
    expect(button).toBeDisabled()
    await user.click(button)
    expect(onClick).not.toHaveBeenCalled()
  })

  it('renders the child element and forwards props when asChild is set', async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    render(
      <Button asChild onClick={onClick}>
        <a href="/next">Next</a>
      </Button>,
    )
    const link = screen.getByRole('link', { name: 'Next' })
    expect(link).toHaveAttribute('data-slot', 'button')
    await user.click(link)
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})

describe('buttonVariants', () => {
  it('returns the merged ghost/sm classes', () => {
    const classes = buttonVariants({ variant: 'ghost', size: 'sm' })
    expect(classes).toContain('hover:bg-accent')
    expect(classes).toContain('h-8')
  })
})
