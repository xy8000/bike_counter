import { expect, test } from '@playwright/test'
import { mapMarkers, SEARCH_TRIGGER_TEXT, waitForStations } from './helpers'

/// Opens the map, picks the first station and returns its id from the URL.
async function openFirstStation(page: import('@playwright/test').Page) {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)
  await mapMarkers(page).first().click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  const stationId = new URL(page.url()).searchParams.get('station')
  expect(stationId).not.toBeNull()
  return stationId as string
}

test('the shared detail page renders the station content and a highlighted map preview', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  // The page content (not blank) and the back navigation.
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  // The overview stat boxes reuse the overview component and add the YEAR stat
  // (scoped to the Overview section: the same text also appears as the
  // "Current year vs. last year" chart legend in the graphs section).
  const overviewSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Overview' }),
  })
  await expect(overviewSection.getByText('Last year', { exact: true })).toBeVisible()
  // The graph sections.
  await expect(page.getByRole('heading', { name: 'Detailed statistics' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Nerd stats' })).toBeVisible()
  // Recharts renders at least one chart.
  await expect(page.locator('.recharts-wrapper').first()).toBeVisible()
  // Exactly one map: the highlighted detail preview (the map view has markers +
  // the sidebar; the detail page has neither).
  await expect(page.locator('.leaflet-container')).toHaveCount(1)
  await expect(page.getByRole('complementary')).toHaveCount(0)
})

test('the header stays active on the detail page', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  // The shared header/search trigger is present and opens the search dialog.
  await expect(page.getByRole('button', { name: SEARCH_TRIGGER_TEXT })).toBeVisible()
  await page.getByRole('button', { name: SEARCH_TRIGGER_TEXT }).click()
  await expect(page.getByPlaceholder('Filter stations by name or description…')).toBeVisible()
})

test('search on the detail page can open another station detail', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  await page.getByRole('button', { name: SEARCH_TRIGGER_TEXT }).click()
  const detailButton = page.getByRole('button', { name: 'Open detail' }).first()
  await expect(detailButton).toBeVisible()
  await detailButton.click()

  // The dialog closes and the selected station's detail page opens.
  await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})

test('the share-by-channel pie renders inside its card', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  // The card title is a `CardTitle` div, so scope by its text on the card.
  const shareCard = page.locator('[data-slot="card"]').filter({ hasText: 'Share by channel' })
  await expect(shareCard).toBeVisible()

  // The pie draws inside the card (regression: the chart used to collapse to
  // zero width and never render). Stations without traffic show the fallback
  // text instead, so only assert the width when a chart is present.
  const wrapper = shareCard.locator('.recharts-wrapper').first()
  if ((await wrapper.count()) > 0) {
    await expect(wrapper).toBeVisible()
    const box = await wrapper.boundingBox()
    expect(box).not.toBeNull()
    expect((box?.width ?? 0) > 0).toBe(true)
  }
})

test('the back-to-map button re-routes to the map and the browser back event returns', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  await page.getByRole('link', { name: 'Back to map' }).click()
  // The map view reports its bounds into the URL.
  await expect(page).toHaveURL(/[?&]min_lat=/)
  await expect(mapMarkers(page).first()).toBeVisible()

  // A browser "back" event re-routes to the station detail page.
  await page.goBack()
  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})

test('clicking the map preview opens the map view at the preview bounds', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const preview = page.locator('.leaflet-container')
  await expect(preview).toBeVisible()
  // Click away from the centred marker (top-left corner of the preview).
  await preview.click({ position: { x: 20, y: 20 } })

  // The map view opens with the visible part of the preview as its bounds.
  await expect(page).toHaveURL(/[?&]min_lat=/)
  await expect(page).toHaveURL(/[?&]max_lng=/)
  await expect(mapMarkers(page).first()).toBeVisible()

  // A browser "back" event re-routes to the station detail page.
  await page.goBack()
  await expect(page).toHaveURL(new RegExp(`/stations/${stationId}`))
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})

test('the shared timeframe selector drives the main chart and the monthly bar chart renders', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const statsSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Detailed statistics' }),
  })

  // The default timeframe is "24 hours" (5-minute buckets).
  await expect(statsSection.getByText('5-minute buckets', { exact: true })).toBeVisible()

  // Switch to "Last 30 days"; the main chart changes (1-day buckets). The Radix
  // Select trigger exposes the combobox role.
  await statsSection.getByRole('combobox', { name: /Timeframe/ }).click()
  await page.getByRole('option', { name: 'Last 30 days' }).click()
  await expect(statsSection.getByText('1-day buckets', { exact: true })).toBeVisible()

  // The standalone monthly bar chart renders with a grand total.
  const monthlyCard = page.locator('[data-slot="card"]').filter({ hasText: 'Bikes per month' })
  await expect(monthlyCard).toBeVisible()
  await expect(monthlyCard.getByText('Total', { exact: true })).toBeVisible()
})

test('the compare-previous checkbox overlays the previous period', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const statsSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Detailed statistics' }),
  })
  const compare = page.getByRole('checkbox', { name: 'Compare previous period' })
  await expect(compare).toBeVisible()

  // Default 24-hours chart is single-series, so no "Day before" legend entry.
  await expect(statsSection.getByText('Day before', { exact: true })).toHaveCount(0)

  // Checking the box overlays the previous period. The previous period appears
  // as a "Day before" legend entry when the current day also has data; a
  // still-importing/older dataset can leave today empty, in which case the
  // previous period draws as the single series (and the legend is intentionally
  // hidden for a single series), so the chart must not stay empty.
  await compare.check()
  const dayBefore = statsSection.getByText('Day before', { exact: true }).first()
  if ((await dayBefore.count()) > 0) {
    await expect(dayBefore).toBeVisible()
  } else {
    await expect(
      statsSection.getByText('No data for this period.', { exact: true }),
    ).toHaveCount(0)
  }
})
