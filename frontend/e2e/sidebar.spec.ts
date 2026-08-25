import { expect, test } from '@playwright/test'
import {
  mapMarkers,
  readSidebarCounts,
  sidebarStationItems,
  waitForStations,
} from './helpers'

test('the sidebar renders only the stations visible in the current viewport', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const baseline = await readSidebarCounts(page)
  const baselineItems = await sidebarStationItems(page).count()
  const baselineMarkers = await mapMarkers(page).count()

  // The counter badge matches the number of rendered sidebar entries…
  expect(baselineItems).toBeGreaterThan(0)
  expect(baseline.visible).toBe(baselineItems)
  // …the markers match the sidebar (same viewport, same positioned set)…
  expect(baselineMarkers).toBe(baselineItems)
  // …and the badge total is the overall station count (>= visible).
  expect(baseline.total).toBeGreaterThanOrEqual(baseline.visible)

  // Focus the map and zoom in twice with the keyboard (the Leaflet zoom control
  // sits under the sidebar overlay). The viewport no longer shows every station.
  await page.locator('.leaflet-container').click({ position: { x: 700, y: 300 } })
  await page.keyboard.press('+')
  await page.keyboard.press('+')

  // Wait for the debounced bounds change + BFF refetch to settle, then assert
  // the visible set shrank while staying internally consistent.
  await expect
    .poll(async () => (await readSidebarCounts(page)).visible)
    .toBeLessThan(baseline.visible)

  const after = await readSidebarCounts(page)
  expect(await sidebarStationItems(page).count()).toBe(after.visible)
  expect(await mapMarkers(page).count()).toBe(after.visible)
})
