import { expect, test } from '@playwright/test'

/// The data-sources pages: the overview lists one section per configured data
/// source (seeded Münster/Bonn/Hamburg) with its counts, and clicking one opens
/// the detail page (image + map + Data-Overview + badges).
test.beforeEach(async ({ page }) => {
  await page.goto('/data-sources')
  await expect(page.getByRole('heading', { name: /Data sources/ })).toBeVisible()
})

test('overview shows one section per data source with counts', async ({ page }) => {
  const cards = page.locator('a[href^="/data-sources/"]')
  await expect(cards).toHaveCount(3)

  await expect(cards.filter({ hasText: 'Münster' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Bonn' })).toBeVisible()
  await expect(cards.filter({ hasText: 'Hamburg' })).toBeVisible()

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
  await expect(page.getByAltText('Münster image')).toBeVisible()
  await expect(page.getByText('Stations', { exact: true })).toBeVisible()
  await expect(page.getByText('Channels', { exact: true })).toBeVisible()
  await expect(page.getByText('First data from', { exact: true })).toBeVisible()
})

test('top-bar link reaches the data-sources overview from the map', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('link', { name: /Data sources/ }).click()
  await expect(page).toHaveURL(/\/data-sources$/)
  await expect(page.getByRole('heading', { name: /Data sources/ })).toBeVisible()
})
