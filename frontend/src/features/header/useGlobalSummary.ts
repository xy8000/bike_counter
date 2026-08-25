import { useEffect, useState } from 'react'
import { fetchGlobalSummary } from './api'
import type { GlobalSummary } from './types'

/// Load the whole-system summary (header) once, on mount.
export function useGlobalSummary() {
  const [summary, setSummary] = useState<GlobalSummary | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    setError(false)
    fetchGlobalSummary()
      .then(setSummary)
      .catch(() => setError(true))
  }, [])

  return { summary, error }
}
