import { safePathSegment } from '../../lib/apiUrl'
import { getJson } from '../../lib/bff'
import type { StationOverviewPage, StationOverviewStats } from './types'

/// Fetch the overview shell (identity) for one counting station. The stats link
/// is derived locally from the validated id, so the shell response never
/// supplies a request destination.
export async function fetchStationOverview(id: string): Promise<StationOverviewPage> {
  const safeId = safePathSegment(id)
  const data = await getJson<Omit<StationOverviewPage, '_links'>>(
    `/api/bff/station-overview/${safeId}`,
  )
  return { ...data, _links: { stats: `/api/bff/station-overview/${safeId}/stats` } }
}

/// Fetch the overview stats card (all-time total + four metrics) for one
/// counting station. The request path is built locally from the validated id, so
/// the shell's `_links.stats` href is never used as a request destination and a
/// compromised shell response cannot steer this request.
export async function fetchStationOverviewStats(id: string): Promise<StationOverviewStats> {
  return getJson<StationOverviewStats>(`/api/bff/station-overview/${safePathSegment(id)}/stats`)
}
