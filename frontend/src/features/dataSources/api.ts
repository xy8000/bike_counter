import { getJson } from '../../lib/bff'
import type { DataSourceDetail, DataSourceImports, DataSourceSummary } from './types'

/// The data-sources overview (one section per configured data source).
export async function fetchDataSources(): Promise<DataSourceSummary[]> {
  const data = await getJson<{ items: DataSourceSummary[] }>('/api/bff/data-sources')
  return data.items
}

/// The detail payload of one data source (stats + badges + map stations).
export async function fetchDataSourceDetail(id: string): Promise<DataSourceDetail> {
  return getJson<DataSourceDetail>(`/api/bff/data-sources/${id}`)
}

/// The import-run history + average duration of one data source.
export async function fetchDataSourceImports(id: string): Promise<DataSourceImports> {
  return getJson<DataSourceImports>(`/api/bff/data-sources/${id}/imports`)
}
