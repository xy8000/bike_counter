import { expect, test } from '@playwright/test'
import { mapMarkers, sidebarBadge, waitForStations } from './helpers'

test('clicking a map marker opens a popup with the station name and detail link', async ({
  page,
}) => {
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
  await expect(popup).toContainText(stationName)
  // The icon button is the explicit detail affordance; the name is also a link.
  await expect(popup.getByRole('link', { name: 'Open detail page' })).toHaveAttribute(
    'href',
    /^\/stations\//
  )
  await expect(popup.getByRole('link', { name: stationName })).toHaveAttribute(
    'href',
    /^\/stations\//
  )

  // The detail link navigates in the same tab: the URL becomes the station
  // detail page and the browser context still holds exactly one page.
  await popup.getByRole('link', { name: 'Open detail page' }).click()
  await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)
  await expect(page.context().pages()).toHaveLength(1)
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})

test('clicking a map marker opens the overview panel and a map void click closes it', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')

  await marker.click()

  // The overview panel replaces the sidebar: it shows the image, the key-fact
  // metrics with trend labels and the detail-page link.
  const overview = page.getByRole('complementary')
  await expect(overview.getByText('Last 24 hours')).toBeVisible()
  await expect(overview.getByText('Last 7 days')).toBeVisible()
  await expect(overview.getByText('Last month')).toBeVisible()
  await expect(overview.getByRole('link', { name: 'Open detail page' })).toHaveAttribute(
    'href',
    /^\/stations\//
  )
  // The heading is clickable (to the same detail page) but not styled as a link.
  await expect(overview.getByRole('link', { name: stationName })).toHaveAttribute(
    'href',
    /^\/stations\//
  )
  await expect(overview.locator('img')).toBeVisible()
  // The old sidebar counter is gone while the overview is open.
  await expect(sidebarBadge(page)).toHaveCount(0)

  // Clicking the map void (far right edge, away from any marker) closes the
  // overview and restores the sidebar.
  const map = page.locator('.leaflet-container')
  const box = await map.boundingBox()
  expect(box).not.toBeNull()
  await map.click({ position: { x: (box?.width ?? 100) - 20, y: (box?.height ?? 100) / 2 } })

  await expect(sidebarBadge(page)).toBeVisible()
})

test('the overview detail link navigates in the same tab', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const marker = mapMarkers(page).first()
  const stationName = (await marker.getAttribute('alt')) ?? ''
  expect(stationName).not.toBe('')

  await marker.click()

  // The overview panel's detail affordance (icon button + name heading) both
  // point at the detail page.
  const overview = page.getByRole('complementary')
  const detailLink = overview.getByRole('link', { name: 'Open detail page' })
  await expect(detailLink).toHaveAttribute('href', /^\/stations\//)
  await expect(overview.getByRole('link', { name: stationName })).toHaveAttribute(
    'href',
    /^\/stations\//,
  )

  // Clicking the icon button navigates in the same tab (no new page).
  await detailLink.click()
  await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)
  await expect(page.context().pages()).toHaveLength(1)
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})
