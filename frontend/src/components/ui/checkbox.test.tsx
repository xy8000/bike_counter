import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Checkbox } from './checkbox'
import { Label } from './label'

describe('Checkbox', () => {
  it('renders a checkbox button with role="checkbox"', () => {
    render(<Checkbox aria-label="Accept terms" />)
    const checkbox = screen.getByRole('checkbox', { name: 'Accept terms' })
    expect(checkbox).toHaveAttribute('data-slot', 'checkbox')
  })

  it('starts unchecked', () => {
    render(<Checkbox aria-label="Accept terms" />)
    expect(screen.getByRole('checkbox')).toHaveAttribute('aria-checked', 'false')
  })

  it('respects defaultChecked', () => {
    render(<Checkbox aria-label="Accept terms" defaultChecked />)
    expect(screen.getByRole('checkbox')).toHaveAttribute('aria-checked', 'true')
  })

  it('fires onCheckedChange(true) when an unchecked checkbox is clicked', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(<Checkbox aria-label="Accept terms" onCheckedChange={onCheckedChange} />)
    await user.click(screen.getByRole('checkbox'))
    expect(onCheckedChange).toHaveBeenCalledWith(true)
  })

  it('fires onCheckedChange(false) when a checked checkbox is clicked', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(<Checkbox aria-label="Accept terms" defaultChecked onCheckedChange={onCheckedChange} />)
    await user.click(screen.getByRole('checkbox'))
    expect(onCheckedChange).toHaveBeenCalledWith(false)
  })

  it('is disabled and does not fire onCheckedChange', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(<Checkbox aria-label="Accept terms" disabled onCheckedChange={onCheckedChange} />)
    const checkbox = screen.getByRole('checkbox')
    expect(checkbox).toBeDisabled()
    await user.click(checkbox)
    expect(onCheckedChange).not.toHaveBeenCalled()
  })

  it('can be toggled by clicking an associated Label', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(
      <div>
        <Checkbox id="terms" onCheckedChange={onCheckedChange} />
        <Label htmlFor="terms">Accept terms</Label>
      </div>,
    )
    await user.click(screen.getByText('Accept terms'))
    expect(onCheckedChange).toHaveBeenCalledWith(true)
  })
})
