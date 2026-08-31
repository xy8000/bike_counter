import { useEffect, useState } from 'react'
import { fetchSummaryGraphs } from './api'
import type { SummaryPeriodGraphs } from './types'

/// Loads one timeframe of the summary graphs card. The URL depends on the
/// shell's HATEOAS link (bounds + `as_of`), the local exclude set and the
/// Bike-Trends flag; the link changes when the timeframe changes, so switching
/// timeframes re-fetches only this card.
export function useStationsSummaryGraphs(
  link: string | null,
  exclude: string[],
  excludeNewStations: boolean,
) {
  const [graphs, setGraphs] = useState<SummaryPeriodGraphs | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)
  const excludeKey = exclude.join(',')

  useEffect(() => {
    if (!link) return
    let cancelled = false
    setLoading(true)
    setGraphs(null)
    setError(false)
    fetchSummaryGraphs(link, exclude, excludeNewStations)
      .then((data) => {
        if (!cancelled) setGraphs(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [link, excludeKey, excludeNewStations])

  return { graphs, loading, error }
}
