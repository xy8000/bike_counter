import { expect, test } from '@playwright/test'
import {
  SEARCH_PLACEHOLDER,
  SEARCH_TRIGGER_TEXT,
  mapMarkers,
  sidebar,
  sidebarBadge,
  sidebarStationItems,
} from './helpers'

const PHONE_VIEWPORT = { width: 390, height: 844 } as const
const TABLET_VIEWPORT = { width: 834, height: 1112 } as const

test.describe('phone layout (390×844)', () => {
  test.use({ viewport: PHONE_VIEWPORT })

  test('the station list starts collapsed and toggles via the floating button', async ({
    page,
  }) => {
    await page.goto('/')
    await expect(mapMarkers(page).first()).toBeVisible()

    // Map-first: the full-screen drawer is closed, so the mobile floating toggle
    // is the only way in (the mid-height handle is hidden below `sm`).
    const open = page.getByRole('button', { name: 'Show station list' })
    await expect(open).toBeVisible()
    await expect(page.getByRole('button', { name: 'Hide station list' })).toHaveCount(0)

    // Open: the drawer covers the map and shows the list + its close button.
    await open.click()
    const close = page.getByRole('button', { name: 'Close station list' })
    await expect(close).toBeVisible()
    await expect(sidebarBadge(page)).toBeVisible()
    await expect(sidebarStationItems(page).first()).toBeVisible()

    // Close: back to the map-first state.
    await close.click()
    await expect(open).toBeVisible()
  })

  test('selecting a station opens the overview inside the drawer', async ({ page }) => {
    await page.goto('/')
    await expect(mapMarkers(page).first()).toBeVisible()

    await page.getByRole('button', { name: 'Show station list' }).click()
    await sidebarStationItems(page).first().click()
    await expect(sidebar(page).getByText('Total bikes (all time)')).toBeVisible()
  })

  test('the overview "Open detailed view" footer button works inside the drawer', async ({
    page,
  }) => {
    await page.goto('/')
    await expect(mapMarkers(page).first()).toBeVisible()

    await page.getByRole('button', { name: 'Show station list' }).click()
    await sidebarStationItems(page).first().click()
    await expect(sidebar(page).getByText('Total bikes (all time)')).toBeVisible()

    // The pinned footer button is reachable inside the full-screen drawer and
    // opens the station's detail page in the same tab.
    const detail = sidebar(page).getByRole('link', { name: 'Open detailed view' })
    await expect(detail).toBeVisible()
    await detail.click()
    await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
  })

  test('the compact header search opens the dialog', async ({ page }) => {
    await page.goto('/')
    // The wide trigger is hidden on phones; the compact icon button (exact
    // name, unlike the wide trigger's trailing ellipsis) opens the dialog.
    await page.getByRole('button', { name: 'Search counting stations', exact: true }).click()
    const input = page.getByPlaceholder(SEARCH_PLACEHOLDER)
    await expect(input).toBeVisible()
    await input.fill('Bismarck')
    await expect(page.locator('li:has(button)').first()).toBeVisible()
  })

  test('the station detail page has no horizontal overflow', async ({ page }) => {
    await page.goto('/')
    await expect(mapMarkers(page).first()).toBeVisible()

    // Drawer → overview → detail, so the deep link is reached through the UI.
    await page.getByRole('button', { name: 'Show station list' }).click()
    await sidebarStationItems(page).first().click()
    await page.getByRole('link', { name: 'Open detail page' }).click()
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()

    const { scrollWidth, clientWidth } = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }))
    expect(scrollWidth).toBeLessThanOrEqual(clientWidth)
  })
})

test.describe('tablet layout (834×1112)', () => {
  test.use({ viewport: TABLET_VIEWPORT })

  test('the drawer is expanded by default with the wide search trigger', async ({ page }) => {
    await page.goto('/')
    await expect(sidebarBadge(page)).toBeVisible()
    await expect(sidebarStationItems(page).first()).toBeVisible()

    // The mid-height pull/push handle is the tablet/desktop affordance.
    await expect(page.getByRole('button', { name: 'Hide station list' })).toBeVisible()
    // The wide search trigger is visible (not the compact icon button).
    await expect(page.getByText(SEARCH_TRIGGER_TEXT)).toBeVisible()
  })
})
