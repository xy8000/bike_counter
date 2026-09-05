import { expect, test } from '@playwright/test'
import {
  CITY_BOUNDS,
  cityUrl,
  mapMarkers,
  SEARCH_PLACEHOLDER,
  SEARCH_TRIGGER_TEXT,
  waitForStations,
} from './helpers'

test('clicking a map marker marks it as selected on the map', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')

  // Before selection the marker shows the normal (active) flag.
  await expect(marker).toHaveClass(/station-marker/)
  await expect(marker).not.toHaveClass(/station-marker--selected/)

  await marker.click()

  // The selected station id lands in the URL and that marker switches to the
  // selected flag (the frontend derives `selected` from the URL station param).
  // `exact` keeps this on the marker img (the overview adds `<img alt="… icon">`
  // and `<img alt="… image">` that would otherwise substring-match).
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  await expect(page.getByAltText(stationName, { exact: true })).toHaveClass(
    /station-marker--selected/,
  )
})

test('a map void click clears the selected flag again', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')
  await marker.click()
  await expect(page.getByAltText(stationName, { exact: true })).toHaveClass(
    /station-marker--selected/,
  )

  // Click the map void (far right edge, away from any marker).
  const map = page.locator('.maplibregl-map')
  const box = await map.boundingBox()
  expect(box).not.toBeNull()
  await map.click({ position: { x: (box?.width ?? 100) - 20, y: (box?.height ?? 100) / 2 } })

  // The station param is dropped and the marker returns to the normal flag.
  await expect(page).not.toHaveURL(/[?&]station=/)
  await expect(page.getByAltText(stationName, { exact: true })).not.toHaveClass(
    /station-marker--selected/,
  )
})

test('search + Find on map marks the found station as selected', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  await page.getByRole('button', { name: SEARCH_TRIGGER_TEXT }).click()
  const input = page.getByPlaceholder(SEARCH_PLACEHOLDER)
  await expect(input).toBeVisible()
  await input.fill('Bohlweg')
  await expect(page.getByRole('button', { name: /Bohlweg/ }).first()).toBeVisible()
  await page.getByRole('button', { name: 'Find on map' }).first().click()

  // The dialog closes and the found station's marker gets the selected flag.
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  await expect(page.getByAltText('Bohlweg', { exact: true })).toHaveClass(
    /station-marker--selected/,
  )
})

// Gartenstraße is marked inactive by the e2e stack after startup
// (scripts/e2e-playwright.sh) to exercise the persisted `status` reporting.
const GARTENSTRASSE_ID = '2b410a8a-3474-4ab3-9fa4-d88b92256bee'

test('an inactive station renders with the inactive flag', async ({ page }) => {
  await page.goto(cityUrl('Münster'), { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  await expect(page.getByAltText('Gartenstraße', { exact: true })).toHaveClass(
    /station-marker--inactive/,
  )
})

test('an inactive station stays inactive even when it is the selected station', async ({
  page,
}) => {
  // Open the map with Gartenstraße selected in the URL: the inactive status must
  // win over the selected state, so the marker stays gray (not amber).
  const bounds = CITY_BOUNDS['Münster']
  const params = new URLSearchParams({
    min_lat: String(bounds.min_lat),
    min_lng: String(bounds.min_lng),
    max_lat: String(bounds.max_lat),
    max_lng: String(bounds.max_lng),
    station: GARTENSTRASSE_ID,
  })
  await page.goto(`/?${params.toString()}`, { waitUntil: 'domcontentloaded' })
  // Selecting a station via the URL opens the overview panel (not the sidebar),
  // so wait for the marker itself instead of `waitForStations`.
  const marker = page.getByAltText('Gartenstraße', { exact: true })
  await expect(marker).toBeVisible()
  await expect(marker).toHaveClass(/station-marker--inactive/)
  await expect(marker).not.toHaveClass(/station-marker--selected/)
})
