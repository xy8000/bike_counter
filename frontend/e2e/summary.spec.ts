import { expect, test, type Page } from '@playwright/test'
import { mapMarkers, sidebar, waitForStations } from './helpers'

const SUMMARIZE_BUTTON = 'Summarize visible stations'

/// A summary URL around the first positioned station. The real map view can
/// aggregate the whole cluster (which is heavy and intentionally shows the
/// loading page), so the tests scope the summary to one station to keep the
/// browser fast and deterministic while still exercising the full feature.
async function smallSummaryUrl(page: Page): Promise<string> {
  const response = await page.request.get('/api/bff/stations/search')
  const data = await response.json()
  const station = (data.items ?? []).find(
    (s: { latitude?: number | null; longitude?: number | null }) =>
      s.latitude != null && s.longitude != null,
  )
  if (!station) throw new Error('no positioned station for the summary e2e')
  const span = 0.002
  const params = new URLSearchParams({
    min_lat: String(station.latitude - span),
    min_lng: String(station.longitude - span),
    max_lat: String(station.latitude + span),
    max_lng: String(station.longitude + span),
  })
  return `/summary?${params.toString()}`
}

/// Wait for the page content to replace the spinner (the aggregation is
/// computed in the backend, so it can take a few seconds).
async function waitForSummaryContent(page: Page) {
  await expect(page.getByRole('heading', { name: 'Station summary' })).toBeVisible({
    timeout: 240_000,
  })
}

test.describe('station summary', () => {
  test.describe.configure({ timeout: 300_000 })

  test('the sidebar opens the shareable station-summary page at the current view', async ({
    page,
  }) => {
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await waitForStations(page)

    // The pinned sidebar footer holds the summarize action (the list above stays
    // scrollable), enabled as soon as there are visible stations.
    const button = sidebar(page).getByRole('button', { name: SUMMARIZE_BUTTON })
    await expect(button).toBeVisible()
    await expect(button).toBeEnabled()
    await button.click()

    // The summary page opens with the map-view bounds in the URL (the same view
    // the user selected) and a back link.
    await expect(page).toHaveURL(/\/summary\?.*min_lat=/)
    await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  })

  test('the summary page renders the fallback image, map and aggregated sections', async ({
    page,
  }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })

    await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
    await expect(page.getByRole('img', { name: 'Station summary image' })).toBeVisible()
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()

    // The aggregated page sections, including the all-time counter.
    await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible()
    await expect(page.getByText('Total bikes (all time)')).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Detailed statistics' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Nerd stats' })).toBeVisible()
    // The hour-of-day radar sits next to the Weekdays radar (split half).
    await expect(
      page.locator('[data-slot="card"]').filter({ hasText: 'Hours' }).first(),
    ).toBeVisible()
    const monthlyCard = page.locator('[data-slot="card"]').filter({ hasText: 'Bikes per month' })
    await expect(monthlyCard).toBeVisible()
    // Recharts draws at least one chart.
    await expect(page.locator('.recharts-wrapper').first()).toBeVisible()
  })

  test('clicking a map flag disables the station and updates the URL', async ({ page }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()

    const marker = mapMarkers(page).first()
    await marker.click()

    // The disabled station id lands in the URL (replace) …
    await expect(page).toHaveURL(/[?&]disabled=[0-9a-f-]+/)
    // … and the clicked marker turns gray once the aggregation re-runs without
    // that station: both the class and the actual computed gray filter.
    await expect(marker).toHaveClass(/leaflet-disabled-marker/, { timeout: 240_000 })
    await expect(marker).toHaveCSS('filter', /grayscale/)
  })

  test('a shared summary URL restores the disabled station as grayed out', async ({ page }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()

    await mapMarkers(page).first().click()
    await expect(page).toHaveURL(/[?&]disabled=[0-9a-f-]+/)
    const sharedUrl = page.url()

    // A fresh page on the shared URL shows the disabled marker grayed.
    await page.goto(sharedUrl, { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()
    const disabledMarker = page.locator('.leaflet-disabled-marker').first()
    await expect(disabledMarker).toBeVisible({ timeout: 240_000 })
    await expect(disabledMarker).toHaveCSS('filter', /grayscale/)
  })

  test('the back-to-map link returns to the map view', async ({ page }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })

    await page.getByRole('link', { name: 'Back to map' }).click()
    await expect(page).toHaveURL(/[?&]min_lat=/)
    await expect(mapMarkers(page).first()).toBeVisible()
    await expect(sidebar(page)).toBeVisible()
  })
})
