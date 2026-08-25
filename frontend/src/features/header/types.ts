/// Whole-system statistics for the header (from GET /api/bff/global-summary).
export interface GlobalSummary {
  station_count: number
  channel_count: number
  bikes_last_24h_total: number
  last_update: string | null
}
