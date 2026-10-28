import { expect, test } from '@playwright/test'

/// The data-sources pages: the overview lists one section per configured data
/// source (seeded all seven: Münster, Bonn, Hamburg, Eco-Counter, Hessen Mobil,
/// Düsseldorf and Köln) with its counts, and clicking one opens the detail page
/// (image + map + Data-Overview + badges).
test.beforeEach(async ({ page }) => {
  await page.goto('/data-sources')
  await expect(page.getByRole('heading', { name: /Data sources/ })).toBeVisible()
})

test('overview shows one section per data source with counts', async ({ page }) => {
  const cards = page.locator('a[href^="/data-sources/"]')
  await expect(cards).toHaveCount(7)

  await expect(cards.filter({ hasText: 'Münster' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Bonn' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Hamburg' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Eco-Counter' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Hessen Mobil' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Düsseldorf' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Stadt Köln' })).toBeVisible()

  // Every section shows a small image (the SVG fallback until a provider
  // serves a logo) and the "Last successful import" label.
  await expect(page.getByText('Last successful import', { exact: false }).first()).toBeVisible()
  await expect(page.locator('a[href^="/data-sources/"] img').first()).toBeVisible()
})

test('clicking a data source opens the detail page', async ({ page }) => {
  await page.getByRole('link', { name: /Münster/ }).click()

  await expect(page).toHaveURL(/\/data-sources\/[0-9a-f-]+/)
  await expect(page.getByRole('heading', { name: /Münster/ })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Data overview' })).toBeVisible()

  // The detail shows the (large) image, the map of provided stations and the
  // station/channel facts.
  await expect(page.getByAltText('Münster', { exact: true })).toBeVisible()
  await expect(page.getByText('Stations', { exact: true })).toBeVisible()
  await expect(page.getByText('Channels', { exact: true })).toBeVisible()
  await expect(page.getByText('First data from', { exact: true })).toBeVisible()
  await expect(page.getByText('Data imported until', { exact: true })).toBeVisible()
})

test('the Import jobs table paginates the seeded runs', async ({ page }) => {
  // Register before the click so the initial page request is not missed.
  const firstPageResponse = page.waitForResponse(
    (response) =>
      response.url().includes('/imports?offset=0&limit=50') && response.status() === 200,
  )

  await page.getByRole('link', { name: /Münster/ }).click()
  await expect(page).toHaveURL(/\/data-sources\/[0-9a-f-]+/)

  // Issue #48: the table renders the newest 50 of the 55 seeded runs by default.
  await expect(page.getByRole('heading', { name: 'Import jobs' })).toBeVisible()
  const rows = page.getByTestId('import-run-row')
  await expect(rows).toHaveCount(50)
  await expect(page.getByText('Showing 1-50 of 55 import jobs')).toBeVisible()
  await expect(page.getByText('Page 1 of 2')).toBeVisible()

  const payload = await (await firstPageResponse).json()
  expect(payload.total).toBe(55)
  expect(payload.items).toHaveLength(50)
  expect(payload.offset).toBe(0)
  expect(payload.limit).toBe(50)
  expect(payload.average_duration_seconds).toBe(30)

  const previous = page.getByRole('button', { name: 'Previous' })
  const next = page.getByRole('button', { name: 'Next' })
  await expect(previous).toBeDisabled()
  await expect(next).toBeEnabled()

  await next.click()
  await expect(page.getByText('Showing 51-55 of 55 import jobs')).toBeVisible()
  await expect(page.getByText('Page 2 of 2')).toBeVisible()
  await expect(rows).toHaveCount(5)
  await expect(previous).toBeEnabled()
  await expect(next).toBeDisabled()

  await previous.click()
  await expect(page.getByText('Showing 1-50 of 55 import jobs')).toBeVisible()
  await expect(rows).toHaveCount(50)
})

test('top-bar link reaches the data-sources overview from the map', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('link', { name: /Data sources/ }).click()
  await expect(page).toHaveURL(/\/data-sources$/)
  await expect(page.getByRole('heading', { name: /Data sources/ })).toBeVisible()
})
