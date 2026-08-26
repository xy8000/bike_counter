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

/// A station summary (from GET /api/bff/stations/search).
export interface StationSummary {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  channel_count: number
  bikes_last_day: number
}

/// A sidebar list entry (from GET /api/bff/stations/sidebar): the station
/// **identity** only — image + name + description + coordinates. The stats live
/// in the separate stats sub-resource so the identity renders immediately.
export interface SidebarStation {
  id: string
  name: string
  description: string
  latitude: number | null
  longitude: number | null
  image_url: string
}

/// The sidebar shell payload: the identities inside the current map view, the
/// visible-vs-global counter and the HATEOAS link to the stats sub-resource.
export interface SidebarShell {
  items: SidebarStation[]
  visible_count: number
  total_count: number
  _links: {
    stats: string
  }
}

/// One station's sidebar stats (from GET /api/bff/stations/sidebar/stats).
export interface SidebarStationStats {
  station_id: string
  channel_count: number
  bikes_last_day: number
}

/// The sidebar stats payload: the stats for every station inside the bounds.
export interface SidebarStats {
  items: SidebarStationStats[]
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
