import { expect, test, type Page } from '@playwright/test'
import { mapMarkers, waitForStations } from './helpers'

/// Resolves the background colour of the element matching `selector` to a 0-255
/// relative-luminance value (via a 1×1 canvas) and reports whether the OS dark
/// preference is active. The canvas step makes the check independent of how the
/// browser serialises the oklch colour (rgb() vs raw token), so it works across
/// engines.
async function readTheme(
  page: Page,
  selector = 'body',
): Promise<{ luminance: number; prefersDark: boolean }> {
  return page.evaluate((sel) => {
    const color = getComputedStyle(document.querySelector(sel)!).backgroundColor
    const canvas = document.createElement('canvas')
    canvas.width = 1
    canvas.height = 1
    const ctx = canvas.getContext('2d')!
    ctx.fillStyle = color
    ctx.fillRect(0, 0, 1, 1)
    const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data
    return {
      luminance: 0.2126 * r + 0.7152 * g + 0.0722 * b,
      prefersDark: window.matchMedia('(prefers-color-scheme: dark)').matches,
    }
  }, selector)
}

/// The dark mode follows the OS colour scheme with no manual toggle: the shadcn
/// dark palette comes from `@media (prefers-color-scheme: dark)` and the map
/// picks `basemap-dark.json` via a `matchMedia` listener in `BaseMap`.
test.describe('dark mode follows the OS colour scheme', () => {
  test('renders dark and loads the dark basemap on a dark system', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' })
    const darkBasemap = page.waitForRequest((request) =>
      request.url().includes('/styles/basemap-dark.json'),
    )
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await waitForStations(page)

    await expect(darkBasemap).resolves.toBeTruthy()
    const theme = await readTheme(page)
    expect(theme.prefersDark).toBe(true)
    expect(theme.luminance).toBeLessThan(64)
  })

  test('renders light and loads the light basemap on a light system', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' })
    const lightBasemap = page.waitForRequest((request) =>
      request.url().includes('/styles/basemap.json'),
    )
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await waitForStations(page)

    await expect(lightBasemap).resolves.toBeTruthy()
    const theme = await readTheme(page)
    expect(theme.prefersDark).toBe(false)
    expect(theme.luminance).toBeGreaterThan(200)
  })

  test('switches to the dark basemap live when the OS preference changes', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' })
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await waitForStations(page)
    await expect(mapMarkers(page).first()).toBeVisible()
    expect((await readTheme(page)).luminance).toBeGreaterThan(200)

    // Flip the OS preference while the page is open: the matchMedia listener in
    // BaseMap must swap the style and the palette must follow.
    const darkBasemap = page.waitForRequest((request) =>
      request.url().includes('/styles/basemap-dark.json'),
    )
    await page.emulateMedia({ colorScheme: 'dark' })
    await expect(darkBasemap).resolves.toBeTruthy()
    const theme = await readTheme(page)
    expect(theme.prefersDark).toBe(true)
    expect(theme.luminance).toBeLessThan(64)
  })

  test('the map popup is themed dark so the station name stays readable', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' })
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await waitForStations(page)

    // The expanded sidebar overlays the left edge; collapse it so the marker is
    // clickable (same pattern as map.spec.ts).
    await page.getByRole('button', { name: 'Hide station list' }).click()

    const marker = mapMarkers(page).first()
    const stationName = (await marker.getAttribute('alt')) ?? ''
    expect(stationName).not.toBe('')
    await marker.click()

    const popup = page.locator('.station-popup')
    await expect(popup).toBeVisible()
    await expect(popup).toContainText(stationName)

    // The popup background must follow the dark palette: if maplibre's white
    // background wins the cascade, the light station name (text-foreground in
    // dark mode) becomes invisible on white.
    const popupTheme = await readTheme(page, '.maplibregl-popup-content')
    expect(popupTheme.prefersDark).toBe(true)
    expect(popupTheme.luminance).toBeLessThan(64)
  })
})
