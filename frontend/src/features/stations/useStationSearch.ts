import { useEffect, useMemo, useState } from 'react'
import { fetchStationSearch } from './api'
import type { ActionDto, StationSummary } from './types'

/// Load every station + the action map once (the search dialog mounts only when
/// open, so this hook runs per dialog session) and expose a client-side
/// name/description filter.
export function useStationSearch() {
  const [query, setQuery] = useState('')
  const [allStations, setAllStations] = useState<StationSummary[] | null>(null)
  const [actions, setActions] = useState<Record<string, ActionDto>>({})
  const [error, setError] = useState(false)

  useEffect(() => {
    fetchStationSearch()
      .then((data) => {
        setAllStations(data.items)
        setActions(data.actions)
      })
      .catch(() => setError(true))
  }, [])

  const results = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    if (!normalized) return allStations ?? []
    return (allStations ?? []).filter(
      (station) =>
        station.name.toLowerCase().includes(normalized) ||
        station.description.toLowerCase().includes(normalized),
    )
  }, [allStations, query])

  // The find-on-map action comes from the BFF action map; for now it is always
  // enabled, but the UI renders it based on the backend contract.
  const findOnMapEnabled = actions.find_on_map?.enabled ?? false

  return { query, setQuery, results, loading: allStations === null, error, findOnMapEnabled }
}
