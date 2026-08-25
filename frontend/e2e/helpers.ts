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

/// The Leaflet marker icons (one per station visible in the current viewport).
export function mapMarkers(page: Page): Locator {
  return page.locator('.leaflet-marker-icon')
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
