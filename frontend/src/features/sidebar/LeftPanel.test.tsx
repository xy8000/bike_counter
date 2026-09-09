import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { LeftPanel } from './LeftPanel'

function renderPanel(overrides: { collapsed?: boolean; onToggle?: () => void } = {}) {
  const { collapsed = false, onToggle = vi.fn() } = overrides
  const utils = render(
    <LeftPanel collapsed={collapsed} onToggle={onToggle}>
      <p>panel content</p>
    </LeftPanel>,
  )
  const aside = utils.container.querySelector('aside')
  if (!aside) throw new Error('expected an <aside> element')
  return { ...utils, onToggle, aside }
}

describe('LeftPanel', () => {
  it('renders its children inside the panel', () => {
    const { aside } = renderPanel()

    expect(screen.getByText('panel content')).toBeInTheDocument()
    expect(aside).toContainElement(screen.getByText('panel content'))
  })

  it('slides the panel off-screen when collapsed', () => {
    const { aside } = renderPanel({ collapsed: true })

    expect(aside.className).toContain('-translate-x-full')
    expect(aside.className).not.toContain('translate-x-0')
  })

  it('keeps the panel on-screen when expanded', () => {
    const { aside } = renderPanel({ collapsed: false })

    expect(aside.className).toContain('translate-x-0')
    expect(aside.className).not.toContain('-translate-x-full')
  })

  it('renders the toggle handle inside the panel and wires it to onToggle', () => {
    const { onToggle } = renderPanel({ collapsed: false })

    const handle = screen.getByRole('button', { name: 'Hide station list' })
    expect(handle).toBeInTheDocument()
    expect(handle).toHaveAttribute('aria-expanded', 'true')

    fireEvent.click(handle)
    expect(onToggle).toHaveBeenCalledTimes(1)
  })

  it('labels the handle for the collapsed state too', () => {
    renderPanel({ collapsed: true })

    expect(screen.getByRole('button', { name: 'Show station list' })).toBeInTheDocument()
  })
})
