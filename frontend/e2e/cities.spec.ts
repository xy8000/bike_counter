import { expect, test } from '@playwright/test'
import { CITY_BOUNDS, CITY_TARGET, cityUrl, mapMarkers } from './helpers'

/// Multi-city map → overview → detail flow, exercised once per seeded city. The
/// e2e stack is seeded from scripts/e2e-seed.sql (no live provider import).
///
/// The overview opens through the app's own shared-link `station` URL param: the
/// map fits the city bounds and opens the selected station's panel. This is
/// robust for dense/co-located markers (e.g. Bonn bridge stations, Hamburg
/// MQ1.2/MQ1.3 a few metres apart), where a bare marker click is intercepted by
/// neighbours. The marker-click interaction itself is covered by map.spec.ts on
/// the Münster view.
for (const city of Object.keys(CITY_BOUNDS)) {
  test(`the ${city} map shows stations and opens overview + detail`, async ({ page }) => {
    const targetName = CITY_TARGET[city]

    // Resolve the seeded target station's id through the search API.
    const response = await page.request.get('/api/bff/stations/search')
    const data = await response.json()
    const station = (data.items ?? []).find((item: { name: string }) => item.name === targetName)
    expect(station).toBeTruthy()

    // Map: load the city view with the station overview open (shared-link URL
    // state), then assert the city's markers render.
    await page.goto(`${cityUrl(city)}&station=${station.id}`, {
      waitUntil: 'domcontentloaded',
    })
    await expect(mapMarkers(page).first()).toBeVisible()

    // Overview: the station overview panel opens with identity + stats.
    const overview = page.getByRole('complementary')
    await expect(overview.getByText(targetName, { exact: true })).toBeVisible()
    await expect(overview.getByText('Total bikes (all time)')).toBeVisible()
    await expect(overview.getByText('Last 24 hours')).toBeVisible()
    await expect(overview.getByRole('link', { name: 'Open detail page' })).toHaveAttribute(
      'href',
      /^\/stations\//,
    )

    // Detail: the detail link opens the station detail page in the same tab,
    // with its highlighted map preview and no sidebar.
    await overview.getByRole('link', { name: 'Open detail page' }).click()
    await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)
    await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
    await expect(page.locator('.maplibregl-map')).toHaveCount(1)
    await expect(page.getByRole('complementary')).toHaveCount(0)
  })
}
