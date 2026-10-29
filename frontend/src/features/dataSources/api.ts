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

/// Default page size of the import-run history (issue #48: the last 50 jobs).
export const IMPORT_PAGE_SIZE = 50

/// How often a running import is polled for its live status and counters.
export const IMPORT_POLL_INTERVAL_MS = 5000

/// One page of the import-run history of a data source, newest first, plus the
/// total count and the average duration over all finished runs.
export async function fetchDataSourceImports(
  id: string,
  offset = 0,
  limit = IMPORT_PAGE_SIZE,
): Promise<DataSourceImports> {
  const params = new URLSearchParams({ offset: String(offset), limit: String(limit) })
  return getJson<DataSourceImports>(`/api/bff/data-sources/${id}/imports?${params.toString()}`)
}
