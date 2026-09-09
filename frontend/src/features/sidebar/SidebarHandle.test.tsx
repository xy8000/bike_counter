import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { SidebarHandle } from './SidebarHandle'

describe('SidebarHandle', () => {
  it('describes the collapsed state and shows a right-pointing icon', () => {
    const { container } = render(<SidebarHandle collapsed onToggle={vi.fn()} />)

    const button = screen.getByRole('button', { name: 'Show station list' })
    expect(button).toHaveAttribute('aria-expanded', 'false')
    expect(button).toHaveAttribute('title', 'Show station list (H)')
    expect(container.querySelector('svg')).not.toBeNull()
  })

  it('describes the expanded state and shows a left-pointing icon', () => {
    const { container } = render(<SidebarHandle collapsed={false} onToggle={vi.fn()} />)

    const button = screen.getByRole('button', { name: 'Hide station list' })
    expect(button).toHaveAttribute('aria-expanded', 'true')
    expect(button).toHaveAttribute('title', 'Hide station list (H)')
    expect(container.querySelector('svg')).not.toBeNull()
  })

  it('fires onToggle when clicked', () => {
    const onToggle = vi.fn()
    render(<SidebarHandle collapsed={false} onToggle={onToggle} />)

    fireEvent.click(screen.getByRole('button', { name: 'Hide station list' }))
    expect(onToggle).toHaveBeenCalledTimes(1)
  })

  it('merges an extra className onto the button', () => {
    render(<SidebarHandle collapsed onToggle={vi.fn()} className="custom-class" />)

    expect(screen.getByRole('button', { name: 'Show station list' }).className).toContain(
      'custom-class',
    )
  })
})
