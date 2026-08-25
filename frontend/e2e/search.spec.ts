import { expect, test } from '@playwright/test'
import { SEARCH_PLACEHOLDER, SEARCH_TRIGGER_TEXT, waitForStations } from './helpers'

test('searching a station and clicking Find on map opens its overview', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  await page.getByRole('button', { name: SEARCH_TRIGGER_TEXT }).click()

  const input = page.getByPlaceholder(SEARCH_PLACEHOLDER)
  await expect(input).toBeVisible()
  await input.fill('Bohlweg')

  const result = page.getByRole('button', { name: /Bohlweg/ }).first()
  await expect(result).toBeVisible()

  await page.getByRole('button', { name: 'Find on map' }).first().click()

  // The dialog closes and the map flies to the station, opening the same
  // overview panel as a marker click (unified selection since plan 33).
  await expect(input).toBeHidden()
  const overview = page.getByRole('complementary')
  await expect(overview.getByText('Last 24 hours')).toBeVisible()
  await expect(overview.getByRole('link', { name: 'Bohlweg' })).toHaveAttribute(
    'href',
    /^\/stations\//,
  )
})
