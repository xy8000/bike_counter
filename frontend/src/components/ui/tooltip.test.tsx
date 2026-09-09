import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { describe, expect, it } from 'vitest'
import { Tooltip, TooltipContent, TooltipTrigger } from './tooltip'

function TooltipHarness() {
  const [open, setOpen] = useState(false)
  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger asChild>
        <button type="button">Hover target</button>
      </TooltipTrigger>
      <TooltipContent>Tooltip copy</TooltipContent>
    </Tooltip>
  )
}

describe('Tooltip', () => {
  it('does not render content while closed', () => {
    render(<TooltipHarness />)
    expect(screen.queryByText('Tooltip copy')).not.toBeInTheDocument()
  })

  it('renders the content when controlled open', async () => {
    render(
      <Tooltip open onOpenChange={() => {}}>
        <TooltipTrigger asChild>
          <button type="button">Hover target</button>
        </TooltipTrigger>
        <TooltipContent>Tooltip copy</TooltipContent>
      </Tooltip>,
    )
    expect(await screen.findByText('Tooltip copy')).toBeInTheDocument()
  })

  it('opens and shows the content when the trigger is hovered', async () => {
    const user = userEvent.setup()
    render(<TooltipHarness />)
    await user.hover(screen.getByRole('button', { name: 'Hover target' }))
    expect(await screen.findByText('Tooltip copy')).toBeInTheDocument()
  })

  it('removes the content when the controlled open state turns false', async () => {
    const user = userEvent.setup()
    function ToggleHarness() {
      const [open, setOpen] = useState(true)
      return (
        <div>
          <Tooltip open={open} onOpenChange={setOpen}>
            <TooltipTrigger asChild>
              <button type="button">Hover target</button>
            </TooltipTrigger>
            <TooltipContent>Tooltip copy</TooltipContent>
          </Tooltip>
          <button type="button" onClick={() => setOpen(false)}>
            Hide
          </button>
        </div>
      )
    }
    render(<ToggleHarness />)
    expect(await screen.findByText('Tooltip copy')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Hide' }))
    await waitFor(() => {
      expect(screen.queryByText('Tooltip copy')).not.toBeInTheDocument()
    })
  })
})
