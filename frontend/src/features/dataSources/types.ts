/// Types for the data-sources pages (BFF `data-sources` + `data-sources/{id}`).

export interface DataSourceSummary {
  id: string
  name: string
  provider_type: string
  /// The last **successful** import of this data source.
  last_updated_at: string | null
  station_count: number
  channel_count: number
  /// URL of the logo content; empty means the provider serves no logo, so the
  /// UI falls back to the bundled data-source SVG.
  image_url: string
  /// The newest per-source import run (status shown in the list).
  last_import: DataSourceImport | null
}

export interface DataSourceMapStation {
  id: string
  name: string
  latitude: number
  longitude: number
  status: 'active' | 'inactive'
}

export interface DataSourceImport {
  status: 'RUNNING' | 'FINISHED' | 'FAILED'
  started_at: string
  finished_at: string | null
  duration_seconds: number | null
  failure_message: string | null
  warning_count: number
  error_count: number
}

export interface DataSourceDetail {
  id: string
  name: string
  provider_type: string
  image_url: string
  station_count: number
  channel_count: number
  /// The positioned counting stations of the data source (map markers).
  stations: DataSourceMapStation[]
  last_updated_at: string | null
  /// Earliest measurement timestamp across the source's channels.
  first_data_at: string | null
  /// Latest measurement timestamp across the source's channels.
  last_data_at: string | null
  has_historical: boolean
  has_real_time: boolean
  has_full_current_year: boolean
  last_import: DataSourceImport | null
}
