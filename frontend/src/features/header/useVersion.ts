import { useEffect, useState } from 'react'
import { fetchBackendVersion } from './api'

/// Load the backend application version for the header version dialog. It is
/// fetched once on mount so the dialog can render it immediately when opened.
export function useVersion() {
  const [backendVersion, setBackendVersion] = useState<string | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    setError(false)
    fetchBackendVersion()
      .then(setBackendVersion)
      .catch(() => setError(true))
  }, [])

  return { backendVersion, error }
}
