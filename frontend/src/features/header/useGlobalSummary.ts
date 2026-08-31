import { useEffect, useState } from 'react'
import { useTrendSettings } from '../settings/TrendSettingsContext'
import { fetchGlobalSummary } from './api'
import type { GlobalSummary } from './types'

/// Load the whole-system summary (header); refetches when the Bike-Trends
/// setting changes so the header reflects the like-for-like total.
export function useGlobalSummary() {
  const { excludeNewStations } = useTrendSettings()
  const [summary, setSummary] = useState<GlobalSummary | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    setError(false)
    fetchGlobalSummary(excludeNewStations)
      .then(setSummary)
      .catch(() => setError(true))
  }, [excludeNewStations])

  return { summary, error }
}
