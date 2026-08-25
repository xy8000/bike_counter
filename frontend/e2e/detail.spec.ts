import { expect, test } from '@playwright/test'
import { mapMarkers, waitForStations } from './helpers'

/// Opens the map, picks the first station and returns its id from the URL.
async function openFirstStation(page: import('@playwright/test').Page) {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)
  await mapMarkers(page).first().click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  const stationId = new URL(page.url()).searchParams.get('station')
  expect(stationId).not.toBeNull()
  return stationId as string
}

test('the shared detail page renders the station content and a highlighted map preview', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  // The page content (not blank) and the back navigation.
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  // The overview stat boxes reuse the overview component and add the YEAR stat
  // (scoped to the Overview section: the same text also appears as the
  // "Current year vs. last year" chart legend in the graphs section).
  const overviewSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Overview' }),
  })
  await expect(overviewSection.getByText('Last year', { exact: true })).toBeVisible()
  // The graph sections.
  await expect(page.getByRole('heading', { name: 'Detailed statistics' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Nerd stats' })).toBeVisible()
  // Recharts renders at least one chart.
  await expect(page.locator('.recharts-wrapper').first()).toBeVisible()
  // Exactly one map: the highlighted detail preview (the map view has markers +
  // the sidebar; the detail page has neither).
  await expect(page.locator('.leaflet-container')).toHaveCount(1)
  await expect(page.getByRole('complementary')).toHaveCount(0)
})

test('the back-to-map button re-routes to the map and the browser back event returns', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  await page.getByRole('link', { name: 'Back to map' }).click()
  // The map view reports its bounds into the URL.
  await expect(page).toHaveURL(/[?&]min_lat=/)
  await expect(mapMarkers(page).first()).toBeVisible()

  // A browser "back" event re-routes to the station detail page.
  await page.goBack()
  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})

test('clicking the map preview opens the map view at the preview bounds', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const preview = page.locator('.leaflet-container')
  await expect(preview).toBeVisible()
  // Click away from the centred marker (top-left corner of the preview).
  await preview.click({ position: { x: 20, y: 20 } })

  // The map view opens with the visible part of the preview as its bounds.
  await expect(page).toHaveURL(/[?&]min_lat=/)
  await expect(page).toHaveURL(/[?&]max_lng=/)
  await expect(mapMarkers(page).first()).toBeVisible()

  // A browser "back" event re-routes to the station detail page.
  await page.goBack()
  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})
