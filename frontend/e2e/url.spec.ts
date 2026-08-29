import { expect, test } from '@playwright/test'
import { mapMarkers, sidebarBadge, waitForStations } from './helpers'

test('the map view URL carries the visible bbox and the open overview', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // Once the map reports its bounds the four bbox params are in the URL, in a
  // finite, ordered form (min < max), so a shared link restores the view.
  await expect(page).toHaveURL(/[?&]min_lat=/)
  await expect(page).toHaveURL(/[?&]max_lng=/)
  const viewUrl = new URL(page.url())
  const minLat = Number(viewUrl.searchParams.get('min_lat'))
  const maxLat = Number(viewUrl.searchParams.get('max_lat'))
  const minLng = Number(viewUrl.searchParams.get('min_lng'))
  const maxLng = Number(viewUrl.searchParams.get('max_lng'))
  expect([minLat, maxLat, minLng, maxLng].every(Number.isFinite)).toBe(true)
  expect(minLat).toBeLessThan(maxLat)
  expect(minLng).toBeLessThan(maxLng)

  // Opening an overview via a marker puts the station id in the URL.
  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')
  await marker.click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)

  // Closing the overview with a map void click drops the station param again.
  const map = page.locator('.maplibregl-map')
  const box = await map.boundingBox()
  expect(box).not.toBeNull()
  await map.click({ position: { x: (box?.width ?? 100) - 20, y: (box?.height ?? 100) / 2 } })
  await expect(page).not.toHaveURL(/[?&]station=/)
  await expect(sidebarBadge(page)).toBeVisible()
})

test('a shared detail URL keeps the station id in the path and renders the detail page', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // Grab a real station id from the URL after opening an overview.
  await mapMarkers(page).first().click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  const stationId = new URL(page.url()).searchParams.get('station')
  expect(stationId).not.toBeNull()

  // The shared detail URL keeps the id in the path and renders the page content
  // (with its highlighted map preview), not the map view / sidebar.
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })
  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  await expect(page.locator('.maplibregl-map')).toHaveCount(1)
  await expect(page.getByRole('complementary')).toHaveCount(0)
})
