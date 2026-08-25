import { expect, test } from '@playwright/test'
import { mapMarkers, waitForStations } from './helpers'

test('clicking a map marker opens a popup with the station name', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // The expanded sidebar overlays the left edge; collapse it so every marker is
  // clickable. This does not change the map bounds (the map is full width).
  await page.getByRole('button', { name: 'Hide station list' }).click()

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')

  await marker.click()

  const popup = page.locator('.leaflet-popup-content')
  await expect(popup).toBeVisible()
  await expect(popup).toHaveText(stationName)
})
