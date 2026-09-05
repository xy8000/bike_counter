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

test('search lists stations alphabetically by name', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  await page.getByRole('button', { name: SEARCH_TRIGGER_TEXT }).click()
  const dialog = page.getByRole('dialog')
  const input = dialog.getByPlaceholder(SEARCH_PLACEHOLDER)
  await expect(input).toBeVisible()

  // Wait for the station list to load (the transient 'Loading stations…' row
  // disappears and result rows render), leaving the full, unfiltered set. A
  // bare `toHaveCount(0)` could pass in the instant before the loading row
  // renders, so poll for the loaded end state instead.
  await expect
    .poll(
      async () => {
        const loading = await dialog.getByText('Loading stations…').count()
        const results = await dialog.locator('li span.font-semibold').count()
        return loading === 0 && results > 0
      },
      { timeout: 10000 },
    )
    .toBe(true)

  // The unfiltered search lists every station; grab the station names of the
  // result rows (each row exposes its name via its font-semibold span).
  const names = await dialog.locator('li').evaluateAll((items) =>
    items
      .map((item) => {
        const name = (item.querySelector('span.font-semibold') as HTMLElement | null)?.textContent
        return name ? name.trim() : null
      })
      .filter((name): name is string => name !== null),
  )
  expect(names.length).toBeGreaterThan(1)

  // Alphabetical order, checked with the browser's own localeCompare so the
  // assertion matches the comparator the app uses to render the list.
  const sortedInBrowser = await page.evaluate(
    (list) => [...list].sort((a, b) => a.localeCompare(b)),
    names,
  )
  expect(names).toEqual(sortedInBrowser)
})
