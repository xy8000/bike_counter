import { expect, type Locator, type Page } from '@playwright/test'

export const SEARCH_TRIGGER_TEXT = 'Search counting stations…'
export const SEARCH_PLACEHOLDER = 'Filter stations by name or description…'

/// The sidebar `<aside>` (implicit role `complementary`). Both the expanded and
/// the collapsed sidebar are `<aside>` elements.
export function sidebar(page: Page): Locator {
  return page.getByRole('complementary')
}

/// The `visible / total` counter badge in the expanded sidebar header.
export function sidebarBadge(page: Page): Locator {
  return sidebar(page).getByText(/^\s*\d+\s*\/\s*\d+\s*$/)
}

/// The station entries in the sidebar: `<li>` elements wrapping a button. The
/// loading / error / empty-state `<li>` elements contain no button, so they are
/// excluded.
export function sidebarStationItems(page: Page): Locator {
  return sidebar(page).locator('li:has(button)')
}

/// The MapLibre station marker images (one per station in the current viewport).
export function mapMarkers(page: Page): Locator {
  return page.locator('.station-marker')
}

export interface VisibleCounts {
  visible: number
  total: number
}

/// Parses the sidebar badge text, e.g. "23 / 26" -> { visible: 23, total: 26 }.
export async function readSidebarCounts(page: Page): Promise<VisibleCounts> {
  const text = (await sidebarBadge(page).textContent())?.trim() ?? ''
  const match = text.match(/^(\d+)\s*\/\s*(\d+)$/)
  if (!match) throw new Error(`Unexpected sidebar badge text: "${text}"`)
  return { visible: Number(match[1]), total: Number(match[2]) }
}

/// Waits until the map has rendered at least one marker and the sidebar shows
/// its counter (i.e. the station import is far enough for the UI to load).
export async function waitForStations(page: Page): Promise<void> {
  await expect(mapMarkers(page).first()).toBeVisible()
  await expect(sidebarBadge(page)).toBeVisible()
}

/// A bounding box per city covering every seeded counting station (see
/// scripts/e2e-seed.sql). Used by the multi-city specs to fly the map to each
/// city. The BFF returns stations name-ordered and the seed guarantees the
/// alphabetically-first station per city has measurements, so the specs' "first
/// marker" per city always renders its charts.
export const CITY_BOUNDS = {
  Münster: { min_lat: 51.8, min_lng: 7.4, max_lat: 52.1, max_lng: 7.9 },
  Bonn: { min_lat: 50.6, min_lng: 7.0, max_lat: 50.8, max_lng: 7.3 },
  Hamburg: { min_lat: 53.4, min_lng: 9.8, max_lat: 53.7, max_lng: 10.2 },
} as const

export type CityName = keyof typeof CITY_BOUNDS

/// The seeded station per city whose marker is isolated enough to click
/// reliably. Some alphabetically-first stations sit a few metres apart (e.g.
/// Hamburg MQ1.2/MQ1.3), where overlapping markers intercept pointer events, so
/// each city targets a marker that is clear of its neighbours. The name matches
/// the marker's `alt`/`title` (see frontend/src/lib/map.tsx stationMarkerImage).
export const CITY_TARGET: Record<CityName, string> = {
  Münster: 'Bismarckallee',
  Bonn: 'BN - Bröltalbahnweg',
  Hamburg: 'MQ10.1+10.2',
}

/// The map-view URL that centres the map on `city` (the map reads the four
/// bounds params from the URL, see frontend/src/lib/geo.ts parseBoundsQuery).
export function cityUrl(city: CityName): string {
  const bounds = CITY_BOUNDS[city]
  const params = new URLSearchParams({
    min_lat: String(bounds.min_lat),
    min_lng: String(bounds.min_lng),
    max_lat: String(bounds.max_lat),
    max_lng: String(bounds.max_lng),
  })
  return `/?${params.toString()}`
}
