import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Input } from './input'

describe('Input', () => {
  it('renders an input with data-slot="input"', () => {
    render(<Input aria-label="name" />)
    const input = screen.getByRole('textbox', { name: 'name' })
    expect(input).toHaveAttribute('data-slot', 'input')
  })

  it('forwards placeholder and type attributes', () => {
    render(<Input type="email" placeholder="you@example.com" />)
    const input = screen.getByRole('textbox')
    expect(input).toHaveAttribute('type', 'email')
    expect(input).toHaveAttribute('placeholder', 'you@example.com')
  })

  it('forwards a controlled value', () => {
    render(<Input value="hello" onChange={() => {}} />)
    expect(screen.getByRole('textbox')).toHaveValue('hello')
  })

  it('fires onChange when the user types', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(<Input aria-label="field" onChange={onChange} />)
    await user.type(screen.getByRole('textbox', { name: 'field' }), 'hi')
    expect(onChange).toHaveBeenCalled()
  })

  it('fires onChange with the new value via fireEvent', () => {
    const onChange = vi.fn()
    render(<Input value="hello" onChange={onChange} />)
    const input = screen.getByRole('textbox')
    fireEvent.change(input, { target: { value: 'world' } })
    expect(onChange).toHaveBeenCalled()
  })

  it('renders as disabled when the disabled prop is set', () => {
    render(<Input aria-label="name" disabled />)
    expect(screen.getByRole('textbox', { name: 'name' })).toBeDisabled()
  })

  it('merges an extra className onto the input', () => {
    render(<Input className="my-extra" aria-label="name" />)
    expect(screen.getByRole('textbox', { name: 'name' })).toHaveClass('my-extra')
  })
})
