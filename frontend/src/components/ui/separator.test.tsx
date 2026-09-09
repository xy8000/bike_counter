import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Separator } from './separator'

function separatorAt(container: HTMLElement) {
  return container.querySelector<HTMLElement>('[data-slot="separator"]')
}

describe('Separator', () => {
  it('renders a horizontal separator by default', () => {
    const { container } = render(<Separator />)
    const separator = separatorAt(container)
    expect(separator).not.toBeNull()
    expect(separator).toHaveAttribute('data-orientation', 'horizontal')
    expect(separator).toHaveAttribute('data-slot', 'separator')
  })

  it('is decorative by default and exposes a separator role when not decorative', () => {
    const { container, rerender } = render(<Separator />)
    expect(separatorAt(container)).toHaveAttribute('role', 'none')
    rerender(<Separator decorative={false} />)
    expect(separatorAt(container)).toHaveAttribute('role', 'separator')
  })

  it('renders a vertical separator when orientation is vertical', () => {
    const { container } = render(<Separator orientation="vertical" />)
    expect(separatorAt(container)).toHaveAttribute('data-orientation', 'vertical')
  })

  it('merges an extra className onto the separator', () => {
    const { container } = render(<Separator className="my-extra" />)
    expect(separatorAt(container)).toHaveClass('my-extra')
  })
})
