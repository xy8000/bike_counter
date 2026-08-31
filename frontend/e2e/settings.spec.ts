import { expect, test, type Page } from '@playwright/test'
import { mapMarkers, waitForStations } from './helpers'

const SETTINGS_BUTTON = 'Calculation settings'
const SETTING_LABEL = /Exclude new stations from trends/

/// Opens the shared station-detail page for the first positioned station and
/// returns its id.
async function openDetailPage(page: Page): Promise<string> {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)
  await mapMarkers(page).first().click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  const stationId = new URL(page.url()).searchParams.get('station')
  expect(stationId).not.toBeNull()
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  return stationId as string
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
  const toggle = dialog.getByRole('switch', { name: SETTING_LABEL })
  await expect(toggle).not.toBeChecked()

  // Enabling the setting makes the header's global summary refetch with the
  // flag appended (the like-for-like total).
  const flaggedRequest = page.waitForRequest(
    (request) =>
      request.url().includes('/api/bff/global-summary') &&
      request.url().includes('exclude_new_stations=true'),
  )
  await toggle.click()
  await expect(toggle).toBeChecked()
  await expect(flaggedRequest).resolves.toBeTruthy()
})

test('the exclude-new-stations flag is stored in the URL and restored on a bare URL', async ({
  page,
}) => {
  const stationId = await openDetailPage(page)
  const dialog = await openSettings(page)
  const toggle = dialog.getByRole('switch', { name: SETTING_LABEL })
  await toggle.click()
  await expect(toggle).toBeChecked()
  await expect(page).toHaveURL(/[?&]exclude_new_stations=1/)

  // A bare URL (no settings params) restores the flag from the cookie and writes
  // it back into the URL, so a shared link carries the filter again.
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  await expect(page).toHaveURL(/[?&]exclude_new_stations=1/)

  // The dialog reflects the restored flag.
  const restoredDialog = await openSettings(page)
  await expect(restoredDialog.getByRole('switch', { name: SETTING_LABEL })).toBeChecked()
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
  await expect(dialog.getByRole('switch', { name: SETTING_LABEL })).toBeVisible()
})

test('the Bike-Trends setting persists across reloads', async ({ page }) => {
  await openDetailPage(page)

  const dialog = await openSettings(page)
  await dialog.getByRole('switch', { name: SETTING_LABEL }).click()

  // A fresh page reads the same value back from localStorage.
  await page.reload({ waitUntil: 'domcontentloaded' })
  const dialogAfterReload = await openSettings(page)
  await expect(dialogAfterReload.getByRole('switch', { name: SETTING_LABEL })).toBeChecked()
})

test('enabling/disabling the setting switches the example graph', async ({ page }) => {
  await openDetailPage(page)

  const dialog = await openSettings(page)
  const toggle = dialog.getByRole('switch', { name: SETTING_LABEL })

  // Off: the "All stations" graph (a station opens partway through the period,
  // so the totals jump) is shown.
  await expect(toggle).not.toBeChecked()
  await expect(dialog.getByText('All stations')).toBeVisible()
  await expect(dialog.getByText('Established only')).toBeHidden()
  await expect(dialog.getByText(/totals jump/)).toBeVisible()

  // Enabling switches to the "Established only" graph.
  await toggle.click()
  await expect(toggle).toBeChecked()
  await expect(dialog.getByText('Established only')).toBeVisible()
  await expect(dialog.getByText('All stations')).toBeHidden()
  await expect(dialog.getByText(/totals grow more evenly/)).toBeVisible()

  // Disabling switches back to "All stations".
  await toggle.click()
  await expect(toggle).not.toBeChecked()
  await expect(dialog.getByText('All stations')).toBeVisible()
  await expect(dialog.getByText('Established only')).toBeHidden()
})

test('the individual option shows two date pickers and disables compare', async ({ page }) => {
  await openDetailPage(page)
  const dialog = await openSettings(page)

  const compare = dialog.getByRole('checkbox', { name: 'Compare previous period' })
  await expect(compare).toBeEnabled()

  await dialog.getByRole('button', { name: 'Individual', exact: true }).click()
  // `exact: true` disambiguates from the "Exclude new stations from trends"
  // switch, which also contains the substring "From".
  await expect(dialog.getByLabel('From', { exact: true })).toBeVisible()
  await expect(dialog.getByLabel('To', { exact: true })).toBeVisible()
  await expect(compare).toBeDisabled()
  await expect(dialog.getByText(/not available for an individual date range/)).toBeVisible()
})

test('switching back from individual restores the prior compare value', async ({ page }) => {
  await openDetailPage(page)
  const dialog = await openSettings(page)
  const compare = dialog.getByRole('checkbox', { name: 'Compare previous period' })
  // The checkbox is controlled by the URL; `.click()` + an explicit wait is
  // more robust than `.check()` against the URL round-trip re-render.
  await compare.click()
  await expect(compare).toBeChecked()

  // Individual keeps the value but disables the box.
  await dialog.getByRole('button', { name: 'Individual', exact: true }).click()
  await expect(compare).toBeDisabled()
  await expect(compare).toBeChecked()

  // Back to a fixed interval: the box is enabled again with the SAME value.
  await dialog.getByRole('button', { name: 'This week', exact: true }).click()
  await expect(compare).toBeEnabled()
  await expect(compare).toBeChecked()
})

test('the individual range is stored in the URL and drives the graph request', async ({ page }) => {
  await openDetailPage(page)
  const dialog = await openSettings(page)
  await dialog.getByRole('button', { name: 'Individual', exact: true }).click()

  const rangeRequest = page.waitForRequest(
    (request) =>
      request.url().includes('/api/bff/station-detail/') &&
      request.url().includes('/graphs/day') &&
      request.url().includes('from=') &&
      request.url().includes('to='),
  )
  // The From/To inputs are controlled by the URL round-trip, so each value must
  // land before the next fill — otherwise the second fill reads stale state and
  // reverts the first input to its default.
  await dialog.getByLabel('From', { exact: true }).fill('2024-01-01')
  await expect(page).toHaveURL(/[?&]from=2024-01-01/)
  await dialog.getByLabel('To', { exact: true }).fill('2024-03-31')
  await expect(page).toHaveURL(/[?&]to=2024-03-31/)
  await expect(rangeRequest).resolves.toBeTruthy()

  // The display dates land in the URL, so a shared link restores the same view.
  await expect(page).toHaveURL(/[?&]timeframe=individual/)
  await expect(page).toHaveURL(/[?&]from=2024-01-01/)
  await expect(page).toHaveURL(/[?&]to=2024-03-31/)
})

test('the settings are restored from the cookie on a bare URL and re-shared', async ({ page }) => {
  const stationId = await openDetailPage(page)
  const dialog = await openSettings(page)
  await dialog.getByRole('button', { name: 'This year', exact: true }).click()
  await dialog.getByRole('button', { name: 'Apply' }).click()
  await expect(page).toHaveURL(/[?&]timeframe=year/)

  // A bare URL (no timeframe param) restores the cookie and writes it back into
  // the URL so the view is shareable again.
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  await expect(page).toHaveURL(/[?&]timeframe=year/)
})
