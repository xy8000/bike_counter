import { expect, test } from '@playwright/test'
import {
  mapMarkers,
  SEARCH_PLACEHOLDER,
  SEARCH_TRIGGER_TEXT,
  stationBoundsUrl,
  stationMarker,
  waitForStations,
  openMap,
} from './helpers'

test('clicking a map marker marks it as selected on the map', async ({ page }) => {
  await openMap(page)

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')

  // Before selection the marker shows the normal (active) flag.
  await expect(marker).toHaveClass(/station-marker/)
  await expect(marker).not.toHaveClass(/station-marker--selected/)

  await marker.click()

  // The selected station id lands in the URL and that marker switches to the
  // selected flag (the frontend derives `selected` from the URL station param).
  // Target the marker by class: the overview banner reuses the station name as
  // its image `alt`, so a bare `getByAltText` would match both.
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  await expect(stationMarker(page, stationName)).toHaveClass(/station-marker--selected/)
})

test('a map void click clears the selected flag again', async ({ page }) => {
  await openMap(page)

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')
  await marker.click()
  await expect(stationMarker(page, stationName)).toHaveClass(/station-marker--selected/)

  // Click the map void (far right edge, away from any marker).
  const map = page.locator('.maplibregl-map')
  const box = await map.boundingBox()
  expect(box).not.toBeNull()
  await map.click({ position: { x: (box?.width ?? 100) - 20, y: (box?.height ?? 100) / 2 } })

  // The station param is dropped and the marker returns to the normal flag.
  await expect(page).not.toHaveURL(/[?&]station=/)
  await expect(stationMarker(page, stationName)).not.toHaveClass(/station-marker--selected/)
})

test('search + Find on map marks the found station as selected', async ({ page }) => {
  await openMap(page)

  await page.getByRole('button', { name: SEARCH_TRIGGER_TEXT }).click()
  const input = page.getByPlaceholder(SEARCH_PLACEHOLDER)
  await expect(input).toBeVisible()
  await input.fill('Bohlweg')
  await expect(page.getByRole('button', { name: /Bohlweg/ }).first()).toBeVisible()
  await page.getByRole('button', { name: 'Find on map' }).first().click()

  // The dialog closes and the found station's marker gets the selected flag.
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  await expect(stationMarker(page, 'Bohlweg')).toHaveClass(/station-marker--selected/)
})

// Gartenstraße is marked inactive by the e2e stack after startup
// (scripts/e2e-playwright.sh) to exercise the persisted `status` reporting.
// Gartenstraße's seeded coordinates (51.9715, 7.6356); the map fits a tight box
// around it so its marker renders individually instead of grouping into a
// cluster circle with a close neighbour at a whole-city view.
const GARTENSTRASSE_ID = '2b410a8a-3474-4ab3-9fa4-d88b92256bee'
const GARTENSTRASSE_COORDS = { latitude: 51.9715, longitude: 7.6356 }

test('an inactive station renders with the inactive flag', async ({ page }) => {
  await page.goto(stationBoundsUrl(GARTENSTRASSE_COORDS.latitude, GARTENSTRASSE_COORDS.longitude), {
    waitUntil: 'domcontentloaded',
  })
  await waitForStations(page)

  await expect(stationMarker(page, 'Gartenstraße')).toHaveClass(/station-marker--inactive/)
})

test('an inactive station stays inactive even when it is the selected station', async ({
  page,
}) => {
  // Open the map with Gartenstraße selected in the URL: the inactive status must
  // win over the selected state, so the marker stays gray (not amber).
  await page.goto(
    `${stationBoundsUrl(GARTENSTRASSE_COORDS.latitude, GARTENSTRASSE_COORDS.longitude)}&station=${GARTENSTRASSE_ID}`,
    { waitUntil: 'domcontentloaded' },
  )
  // Selecting a station via the URL opens the overview panel (not the sidebar),
  // so wait for the marker itself instead of `waitForStations`.
  const marker = stationMarker(page, 'Gartenstraße')
  await expect(marker).toBeVisible()
  await expect(marker).toHaveClass(/station-marker--inactive/)
  await expect(marker).not.toHaveClass(/station-marker--selected/)
})
