import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from './dialog'

function DialogBody() {
  return (
    <>
      <DialogHeader>
        <DialogTitle>Settings</DialogTitle>
        <DialogDescription>Manage your settings here.</DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <button type="button">Save</button>
      </DialogFooter>
    </>
  )
}

describe('Dialog', () => {
  it('renders controlled content in the document when open', async () => {
    render(
      <Dialog open onOpenChange={() => {}}>
        <DialogContent>
          <DialogBody />
        </DialogContent>
      </Dialog>,
    )
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toBeInTheDocument()
    expect(screen.getByText('Settings')).toBeInTheDocument()
    expect(screen.getByText('Manage your settings here.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Save' })).toBeInTheDocument()
  })

  it('does not render content when closed', () => {
    render(
      <Dialog open={false} onOpenChange={() => {}}>
        <DialogContent>
          <DialogBody />
        </DialogContent>
      </Dialog>,
    )
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(screen.queryByText('Settings')).not.toBeInTheDocument()
  })

  it('opens through a DialogTrigger with asChild', async () => {
    const user = userEvent.setup()
    function TriggerHarness() {
      const [open, setOpen] = useState(false)
      return (
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogTrigger asChild>
            <button type="button">Open dialog</button>
          </DialogTrigger>
          <DialogContent>
            <DialogBody />
          </DialogContent>
        </Dialog>
      )
    }
    render(<TriggerHarness />)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Open dialog' }))
    expect(await screen.findByRole('dialog')).toBeInTheDocument()
  })

  it('calls onOpenChange(false) when the close button is clicked', async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogBody />
        </DialogContent>
      </Dialog>,
    )
    await screen.findByRole('dialog')
    await user.click(screen.getByRole('button', { name: 'Close' }))
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it('calls onOpenChange(false) when Escape is pressed', async () => {
    const onOpenChange = vi.fn()
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogBody />
        </DialogContent>
      </Dialog>,
    )
    await screen.findByRole('dialog')
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it('calls onOpenChange(false) when the overlay is clicked', async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogBody />
        </DialogContent>
      </Dialog>,
    )
    await screen.findByRole('dialog')
    // DialogContent renders through a portal, so the overlay lives on document.body.
    const overlay = document.querySelector<HTMLElement>('[data-slot="dialog-overlay"]')
    expect(overlay).not.toBeNull()
    await user.click(overlay as HTMLElement)
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it('hides the close button when showCloseButton is false', async () => {
    render(
      <Dialog open onOpenChange={() => {}}>
        <DialogContent showCloseButton={false}>
          <DialogBody />
        </DialogContent>
      </Dialog>,
    )
    await screen.findByRole('dialog')
    expect(screen.queryByRole('button', { name: 'Close' })).not.toBeInTheDocument()
  })
})
