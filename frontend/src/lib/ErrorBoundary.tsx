import { Component, type ErrorInfo, type ReactNode } from 'react'

interface ErrorBoundaryProps {
  fallback?: ReactNode
  children: ReactNode
}

interface ErrorBoundaryState {
  error: Error | null
}

/// Catches rendering errors below it and swaps in a small inline fallback
/// instead of letting React unmount the whole tree (which would blank the page
/// and leave the header/search unusable — the failure mode we saw with an empty
/// Recharts radar). Errors are also surfaced to the console for debugging.
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('ErrorBoundary caught an error:', error, info)
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        this.props.fallback ?? (
          <div className="flex items-center justify-center p-6">
            <p className="text-sm text-muted-foreground">
              This section failed to load. Reload the page to try again.
            </p>
          </div>
        )
      )
    }
    return this.props.children
  }
}
