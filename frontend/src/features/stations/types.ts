/// Minimal map marker: only what the map needs (from GET /api/bff/stations).
export interface StationMap {
  id: string
  name: string
  latitude: number
  longitude: number
}

export interface StationMapList {
  items: StationMap[]
}

/// A station summary (from GET /api/bff/stations/sidebar and .../search).
export interface StationSummary {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  channel_count: number
  bikes_last_24h: number
}

/// Sidebar payload: summaries + the visible/global counter.
export interface StationSummarySidebar {
  items: StationSummary[]
  visible_count: number
  total_count: number
}

/// A possible action offered by the search dialog (from .../search).
export interface ActionDto {
  enabled: boolean
}

/// Search payload: every station plus the map of possible actions.
export interface StationSearch {
  items: StationSummary[]
  actions: Record<string, ActionDto>
}
