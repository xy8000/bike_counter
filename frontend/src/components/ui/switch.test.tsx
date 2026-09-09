import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Switch } from './switch'

describe('Switch', () => {
  it('renders a switch with role="switch" and data-slot="switch"', () => {
    render(<Switch aria-label="Notifications" />)
    const sw = screen.getByRole('switch', { name: 'Notifications' })
    expect(sw).toHaveAttribute('data-slot', 'switch')
  })

  it('starts unchecked', () => {
    render(<Switch aria-label="Notifications" />)
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'false')
  })

  it('reflects a controlled checked state', () => {
    render(<Switch aria-label="Notifications" checked />)
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'true')
  })

  it('fires onCheckedChange(true) when toggled on', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(<Switch aria-label="Notifications" onCheckedChange={onCheckedChange} />)
    await user.click(screen.getByRole('switch'))
    expect(onCheckedChange).toHaveBeenCalledWith(true)
  })

  it('fires onCheckedChange(false) when a checked switch is clicked', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(<Switch aria-label="Notifications" defaultChecked onCheckedChange={onCheckedChange} />)
    const sw = screen.getByRole('switch')
    expect(sw).toHaveAttribute('aria-checked', 'true')
    await user.click(sw)
    expect(onCheckedChange).toHaveBeenCalledWith(false)
  })

  it('is disabled and does not fire onCheckedChange', async () => {
    const user = userEvent.setup()
    const onCheckedChange = vi.fn()
    render(<Switch aria-label="Notifications" disabled onCheckedChange={onCheckedChange} />)
    const sw = screen.getByRole('switch')
    expect(sw).toBeDisabled()
    await user.click(sw)
    expect(onCheckedChange).not.toHaveBeenCalled()
  })
})
