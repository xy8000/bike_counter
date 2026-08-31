import { expect, test, type Page } from '@playwright/test'
import { mapMarkers, waitForStations } from './helpers'

const SETTINGS_BUTTON = 'Calculation settings'
const SETTING_LABEL = /Exclude new stations from trends/

/// Opens the shared station-detail page for the first positioned station.
async function openDetailPage(page: Page): Promise<void> {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)
  await mapMarkers(page).first().click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  const stationId = new URL(page.url()).searchParams.get('station')
  expect(stationId).not.toBeNull()
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
}

/// Opens the Bike-Trends settings dialogue via the page-level button.
async function openSettings(page: Page) {
  await page.getByRole('button', { name: SETTINGS_BUTTON }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('heading', { name: 'Bike-Trends settings' })).toBeVisible()
  return dialog
}

test('the detail-page settings button toggles the exclude-new-stations flag', async ({ page }) => {
  await openDetailPage(page)

  const dialog = await openSettings(page)
  const checkbox = dialog.getByRole('checkbox', { name: SETTING_LABEL })
  await expect(checkbox).not.toBeChecked()

  // Enabling the setting makes the header's global summary refetch with the
  // flag appended (the like-for-like total).
  const flaggedRequest = page.waitForRequest(
    (request) =>
      request.url().includes('/api/bff/global-summary') &&
      request.url().includes('exclude_new_stations=true'),
  )
  await checkbox.check()
  await expect(checkbox).toBeChecked()
  await expect(flaggedRequest).resolves.toBeTruthy()
})

test('the summary-page settings button opens the same dialogue', async ({ page }) => {
  // A summary URL around the first positioned station, like summary.spec.ts.
  const response = await page.request.get('/api/bff/stations/search')
  const data = await response.json()
  const station = (data.items ?? []).find(
    (s: { latitude?: number | null; longitude?: number | null }) =>
      s.latitude != null && s.longitude != null,
  )
  if (!station) throw new Error('no positioned station for the settings e2e')
  const span = 0.002
  const params = new URLSearchParams({
    min_lat: String(station.latitude - span),
    min_lng: String(station.longitude - span),
    max_lat: String(station.latitude + span),
    max_lng: String(station.longitude + span),
  })
  await page.goto(`/summary?${params.toString()}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('heading', { name: 'Station summary' })).toBeVisible({
    timeout: 240_000,
  })

  const dialog = await openSettings(page)
  await expect(dialog.getByRole('checkbox', { name: SETTING_LABEL })).toBeVisible()
})

test('the Bike-Trends setting persists across reloads', async ({ page }) => {
  await openDetailPage(page)

  const dialog = await openSettings(page)
  await dialog.getByRole('checkbox', { name: SETTING_LABEL }).check()

  // A fresh page reads the same value back from localStorage.
  await page.reload({ waitUntil: 'domcontentloaded' })
  const dialogAfterReload = await openSettings(page)
  await expect(dialogAfterReload.getByRole('checkbox', { name: SETTING_LABEL })).toBeChecked()
})
