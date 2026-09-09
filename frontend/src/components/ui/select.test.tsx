import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select'

// Radix Select tracks pointer capture while open/selecting; jsdom does not
// implement the pointer-capture API, so shim it before any Radix event runs.
if (typeof Element.prototype.hasPointerCapture !== 'function') {
  Element.prototype.hasPointerCapture = () => false
}
if (typeof Element.prototype.setPointerCapture !== 'function') {
  Element.prototype.setPointerCapture = () => {}
}
if (typeof Element.prototype.releasePointerCapture !== 'function') {
  Element.prototype.releasePointerCapture = () => {}
}

function FruitSelect({
  onValueChange,
  defaultValue,
}: {
  onValueChange?: (value: string) => void
  defaultValue?: string
}) {
  return (
    <Select onValueChange={onValueChange} defaultValue={defaultValue}>
      <SelectTrigger aria-label="fruit">
        <SelectValue placeholder="Pick a fruit" />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="apple">Apple</SelectItem>
        <SelectItem value="banana">Banana</SelectItem>
      </SelectContent>
    </Select>
  )
}

describe('Select', () => {
  it('renders a trigger with the placeholder while nothing is selected', () => {
    render(<FruitSelect />)
    expect(screen.getByRole('combobox', { name: 'fruit' })).toBeInTheDocument()
    expect(screen.getByText('Pick a fruit')).toBeInTheDocument()
  })

  it('shows the defaultValue in the trigger', () => {
    render(<FruitSelect defaultValue="banana" />)
    expect(screen.getByRole('combobox', { name: 'fruit' })).toHaveTextContent('Banana')
  })

  it('lists the items once opened', async () => {
    const user = userEvent.setup()
    render(<FruitSelect />)
    await user.click(screen.getByRole('combobox', { name: 'fruit' }))
    const options = await screen.findAllByRole('option')
    expect(options.map((o) => o.textContent)).toEqual(expect.arrayContaining(['Apple', 'Banana']))
  })

  it('fires onValueChange with the selected value and shows it in the trigger', async () => {
    const user = userEvent.setup()
    const onValueChange = vi.fn()
    render(<FruitSelect onValueChange={onValueChange} />)
    await user.click(screen.getByRole('combobox', { name: 'fruit' }))
    const apple = await screen.findByRole('option', { name: 'Apple' })
    await user.click(apple)
    expect(onValueChange).toHaveBeenCalledWith('apple')
    expect(screen.getByRole('combobox', { name: 'fruit' })).toHaveTextContent('Apple')
  })
})
