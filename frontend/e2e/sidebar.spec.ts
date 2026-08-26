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

  // Focus the map and zoom in with the keyboard (the Leaflet zoom control sits
  // at the top-right, clear of the sidebar). After each moveend the map
  // re-renders and can drop a keypress, so keep zooming until the visible set
  // actually shrinks.
  const map = page.locator('.leaflet-container')
  // The focus click must land on map "void", not a marker: a marker hit opens
  // the station overview, which replaces the sidebar and breaks the count
  // assertions. Click a far corner, well clear of the central Münster cluster.
  await map.click({ position: { x: 1200, y: 620 } })
  // Guard: if the click still opened the overview, close it so the sidebar
  // badge is readable again before the zoom poll.
  const closeOverview = page.getByRole('button', { name: 'Close station overview' })
  if (await closeOverview.isVisible().catch(() => false)) {
    await closeOverview.click()
  }
  await map.focus()
  await expect
    .poll(
      async () => {
        const current = (await readSidebarCounts(page)).visible
        if (current >= baseline.visible) {
          await page.keyboard.press('+')
          await page.waitForTimeout(400)
        }
        return (await readSidebarCounts(page)).visible
      },
      { timeout: 20000 },
    )
    .toBeLessThan(baseline.visible)

  // After zooming, the visible set shrank and stays internally consistent.
  const after = await readSidebarCounts(page)
  await expect(sidebarStationItems(page)).toHaveCount(after.visible)
  await expect(mapMarkers(page)).toHaveCount(after.visible)
})
