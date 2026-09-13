/// Shared fixtures for the station-summary tests (`api.test.ts` and
/// `useStationsSummaryPage.test.tsx` both stub the same shell payload). Keeping
/// them here removes the duplicated fixture blocks SonarCloud reported.
import type { Bounds } from '@/lib/geo'

/// The bounds the summary tests request.
export const SUMMARY_BOUNDS: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }

/// A second, disjoint bounds set used to assert a refetch on a bounds change.
export const OTHER_SUMMARY_BOUNDS: Bounds = { min_lat: 52, min_lng: 8, max_lat: 53, max_lng: 9 }

/// The raw summary shell payload as the BFF serves it: each `_links` value is a
/// `LinkDto` (`{ href, templated }`) that the api layer unwraps to a bare href.
export function rawSummaryPage() {
  return {
    image_url: '/img/summary.png',
    stations: [],
    last_update: null,
    _links: {
      self: { href: '/api/summary/self', templated: false },
      overview: { href: '/api/overview/summary', templated: false },
      graphs_day: { href: '/api/graphs/summary/day', templated: false },
      graphs_week: { href: '/api/graphs/summary/week', templated: false },
      graphs_last_30_days: { href: '/api/graphs/summary/last_30_days', templated: false },
      graphs_year: { href: '/api/graphs/summary/year', templated: false },
      monthly: { href: '/api/monthly/summary', templated: false },
    },
  }
}
