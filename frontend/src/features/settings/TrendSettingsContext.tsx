import { createContext, useContext, useEffect, useState, type ReactNode } from 'react'

/// localStorage key for the Bike-Trends "exclude new stations" setting.
const STORAGE_KEY = 'bike-counter.trends.exclude_new_stations'

interface TrendSettingsValue {
  /// When on, trend metrics and comparison graphs only include stations that
  /// have data covering the whole compared period (see the backend
  /// `exclude_new_stations` query parameter).
  excludeNewStations: boolean
  setExcludeNewStations: (value: boolean) => void
}

const TrendSettingsContext = createContext<TrendSettingsValue | null>(null)

function readStored(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

/// App-wide Bike-Trends settings, persisted in localStorage so the choice
/// survives reloads and is shared by the header (global summary), the summary
/// page and the station detail page.
export function TrendSettingsProvider({ children }: { children: ReactNode }) {
  const [excludeNewStations, setExcludeNewStations] = useState<boolean>(readStored)

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, String(excludeNewStations))
    } catch {
      // localStorage unavailable (e.g. private mode): keep the in-memory value.
    }
  }, [excludeNewStations])

  return (
    <TrendSettingsContext.Provider value={{ excludeNewStations, setExcludeNewStations }}>
      {children}
    </TrendSettingsContext.Provider>
  )
}

export function useTrendSettings(): TrendSettingsValue {
  const value = useContext(TrendSettingsContext)
  if (!value) {
    throw new Error('useTrendSettings must be used within a TrendSettingsProvider')
  }
  return value
}
