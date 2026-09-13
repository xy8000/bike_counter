import { expect, test } from '@playwright/test'
import { expectTheme, firstMarker, mapMarkers, openMap } from './helpers'

/// The dark mode follows the OS colour scheme with no manual toggle: the shadcn
/// dark palette comes from `@media (prefers-color-scheme: dark)` and the map
/// picks `basemap-dark.json` via a `matchMedia` listener in `BaseMap`.
test.describe('dark mode follows the OS colour scheme', () => {
  test('renders dark and loads the dark basemap on a dark system', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' })
    const darkBasemap = page.waitForRequest((request) =>
      request.url().includes('/styles/basemap-dark.json'),
    )
    await openMap(page)

    await expect(darkBasemap).resolves.toBeTruthy()
    await expectTheme(page, 'dark')
  })

  test('renders light and loads the light basemap on a light system', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' })
    const lightBasemap = page.waitForRequest((request) =>
      request.url().includes('/styles/basemap.json'),
    )
    await openMap(page)

    await expect(lightBasemap).resolves.toBeTruthy()
    await expectTheme(page, 'light')
  })

  test('switches to the dark basemap live when the OS preference changes', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' })
    await openMap(page)
    await expect(mapMarkers(page).first()).toBeVisible()
    await expectTheme(page, 'light')

    // Flip the OS preference while the page is open: the matchMedia listener in
    // BaseMap must swap the style and the palette must follow.
    const darkBasemap = page.waitForRequest((request) =>
      request.url().includes('/styles/basemap-dark.json'),
    )
    await page.emulateMedia({ colorScheme: 'dark' })
    await expect(darkBasemap).resolves.toBeTruthy()
    await expectTheme(page, 'dark')
  })

  test('the map popup is themed dark so the station name stays readable', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' })
    await openMap(page)

    // The expanded sidebar overlays the left edge; collapse it so the marker is
    // clickable (same pattern as map.spec.ts).
    await page.getByRole('button', { name: 'Hide station list' }).click()

    const { marker, stationName } = await firstMarker(page)
    await marker.click()

    const popup = page.locator('.station-popup')
    await expect(popup).toBeVisible()
    await expect(popup).toContainText(stationName)

    // The popup background must follow the dark palette: if maplibre's white
    // background wins the cascade, the light station name (text-foreground in
    // dark mode) becomes invisible on white.
    await expectTheme(page, 'dark', '.maplibregl-popup-content')
  })
})
