import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ErrorBoundary } from './ErrorBoundary'

/// A child that throws while rendering, to exercise the boundary's catch path.
/// The explicit `ReactNode` return type keeps it a valid JSX component under the
/// strict `tsc` gate (an untyped throw-only function narrows to `never`/`void`,
/// which React 19's JSX typing rejects).
function Bomb(): ReactNode {
  throw new Error('boom')
}

function Happy(): ReactNode {
  return <p>happy child</p>
}

function renderBoundary(ui: ReactNode) {
  const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
  const utils = render(ui)
  return { ...utils, errorSpy }
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe('ErrorBoundary', () => {
  it('renders its children when nothing throws', () => {
    renderBoundary(
      <ErrorBoundary>
        <Happy />
      </ErrorBoundary>,
    )

    expect(screen.getByText('happy child')).toBeInTheDocument()
  })

  it('swaps in the default fallback when a child throws while rendering', () => {
    renderBoundary(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>,
    )

    expect(
      screen.getByText('This section failed to load. Reload the page to try again.'),
    ).toBeInTheDocument()
    expect(screen.queryByText('happy child')).not.toBeInTheDocument()
  })

  it('renders the custom fallback prop when a child throws', () => {
    renderBoundary(
      <ErrorBoundary fallback={<p>custom fallback</p>}>
        <Bomb />
      </ErrorBoundary>,
    )

    expect(screen.getByText('custom fallback')).toBeInTheDocument()
    expect(
      screen.queryByText('This section failed to load. Reload the page to try again.'),
    ).not.toBeInTheDocument()
  })

  it('logs the caught error through componentDidCatch', () => {
    const { errorSpy } = renderBoundary(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>,
    )

    expect(errorSpy).toHaveBeenCalledWith(
      'ErrorBoundary caught an error:',
      expect.any(Error),
      expect.any(Object),
    )
  })

  it('keeps the fallback permanently once an error has been caught', () => {
    // getDerivedStateFromError turns the error into state, so re-rendering a
    // different (now happy) child must not recover to the children.
    const { rerender } = renderBoundary(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>,
    )
    rerender(
      <ErrorBoundary>
        <Happy />
      </ErrorBoundary>,
    )

    expect(
      screen.getByText('This section failed to load. Reload the page to try again.'),
    ).toBeInTheDocument()
    expect(screen.queryByText('happy child')).not.toBeInTheDocument()
  })
})
