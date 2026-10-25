import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { FRONTEND_VERSION } from '../../lib/version'
import { VersionDialog } from './VersionDialog'

describe('VersionDialog', () => {
  it('renders the title and both versions with a leading v', () => {
    render(<VersionDialog backendVersion="0.0.9" error={false} onClose={vi.fn()} />)

    expect(screen.getByRole('heading', { name: 'Versions' })).toBeInTheDocument()
    expect(screen.getByText('Currently installed application versions.')).toBeInTheDocument()

    expect(screen.getByText('Frontend')).toBeInTheDocument()
    expect(screen.getByText(`v${FRONTEND_VERSION}`)).toBeInTheDocument()
    expect(screen.getByText('Backend')).toBeInTheDocument()
    expect(screen.getByText('v0.0.9')).toBeInTheDocument()
  })

  it('shows a loading label while the backend version is pending', () => {
    render(<VersionDialog backendVersion={null} error={false} onClose={vi.fn()} />)

    expect(screen.getByText('Loading…')).toBeInTheDocument()
    expect(screen.queryByText('Unavailable')).not.toBeInTheDocument()
  })

  it('shows an unavailable label when the backend version fails to load', () => {
    render(<VersionDialog backendVersion={null} error onClose={vi.fn()} />)

    expect(screen.getByText('Unavailable')).toBeInTheDocument()
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument()
  })

  it('invokes onClose when the dialog is dismissed', () => {
    const onClose = vi.fn()
    render(<VersionDialog backendVersion="0.0.8" error={false} onClose={onClose} />)

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
