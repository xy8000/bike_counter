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
  // The page shell renders first (back link + highlighted map preview); the
  // per-card stats sub-resources load afterwards (Playwright auto-waits).
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  // The overview section shows the all-time counter, and the stat boxes reuse
  // the overview component and add the YEAR stat (scoped to the Overview
  // section: the same text also appears as the "Current year vs. last year"
  // chart legend in the graphs section).
  const overviewSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Overview' }),
  })
  await expect(overviewSection.getByText('Total bikes (all time)')).toBeVisible()
  await expect(overviewSection.getByText('Last year', { exact: true })).toBeVisible()
  // The graph sections.
  await expect(page.getByRole('heading', { name: 'Detailed statistics' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Detailed stats' })).toBeVisible()
  // Recharts renders at least one chart.
  await expect(page.locator('.recharts-wrapper').first()).toBeVisible()
  // Exactly one map: the highlighted detail preview (the map view has markers +
  // the sidebar; the detail page has neither).
  await expect(page.locator('.maplibregl-map')).toHaveCount(1)
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

test('the hour-of-day radar renders next to the Weekdays radar', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  // Aggregate "Hours" radar (first card; "Hours by channel" also matches the
  // substring filter) sits in the Detailed statistics section next to Weekdays.
  const hoursCard = page.locator('[data-slot="card"]').filter({ hasText: 'Hours' }).first()
  await expect(hoursCard).toBeVisible()

  // The radar draws inside the card when there is traffic; stations without
  // traffic show the fallback text instead, so only assert when a chart exists.
  const wrapper = hoursCard.locator('.recharts-wrapper').first()
  if ((await wrapper.count()) > 0) {
    await expect(wrapper).toBeVisible()
  }

  // The detailed-stats variant exists too.
  await expect(
    page.locator('[data-slot="card"]').filter({ hasText: 'Hours by channel' }),
  ).toBeVisible()
})

test('the detailed statistics section shows key facts for the selected timeframe', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const statsSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Detailed statistics' }),
  })

  // The key facts are derived from the selected timeframe's graph data (default
  // "This week") and use the overview's metric-box theme.
  await expect(statsSection.getByText('Total bikes in selection', { exact: true })).toBeVisible()
  await expect(statsSection.getByText('Busiest day in range', { exact: true })).toBeVisible()
  await expect(statsSection.getByText('Busiest hour', { exact: true })).toBeVisible()
  await expect(statsSection.getByText('Busiest weekday', { exact: true })).toBeVisible()
})

test('the bikes-per-month chart is the last section and shows the settings hint', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  // The monthly chart moved below "Detailed stats", so it is the last section.
  const monthlySection = page.locator('main section').filter({
    has: page.getByRole('heading', { name: 'Bikes per month' }),
  })
  await expect(monthlySection).toBeVisible()
  await expect(page.locator('main section').last()).toContainText('Bikes per month')

  // The section carries its own heading (the card itself has no duplicate
  // title) and the remark underneath explains that the settings are not applied
  // to this chart, because newly added counting stations may add bikes.
  await expect(monthlySection.getByText(/settings are not applied to this chart/i)).toBeVisible()
  await expect(monthlySection.locator('[data-slot="card"]')).toBeVisible()
})

// Gasselstiege (Münster) has 6 channels; the e2e fixture synthesizes data for
// all of them so every per-channel nerd-stats chart exceeds the 5-stream limit.
const GASSELSTIEGE_ID = '97514fa2-2a21-4a17-b85c-6ec4aa74db27'

test('the per-channel nerd-stats charts show the info note for a station with more than 5 channels', async ({
  page,
}) => {
  await page.goto(`/stations/${GASSELSTIEGE_ID}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()

  // The per-channel line chart is replaced by the info note and draws nothing.
  const lineCard = page.locator('[data-slot="card"]').filter({ hasText: 'This week by channel' })
  await expect(lineCard.getByText(/too many data-streams to render/)).toBeVisible()
  await expect(lineCard.locator('.recharts-wrapper')).toHaveCount(0)

  // The per-channel weekday radar and the share pie are guarded the same way.
  const weekdayCard = page.locator('[data-slot="card"]').filter({ hasText: 'Weekdays by channel' })
  await expect(weekdayCard.getByText(/too many data-streams to render/)).toBeVisible()
  const shareCard = page.locator('[data-slot="card"]').filter({ hasText: 'Share by channel' })
  await expect(shareCard.getByText(/too many data-streams to render/)).toBeVisible()

  // The aggregate "Detailed statistics" section is unaffected and still draws.
  const statsSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Detailed statistics' }),
  })
  await expect(statsSection.locator('.recharts-wrapper').first()).toBeVisible()
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

test('back to map after an in-app detail navigation restores the previous view', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // Open the overview via a marker (this flies to the station and puts the id
  // in the URL). The fly-to duration scales with the flight distance, so wait
  // for the URL to stabilise (the moveend-driven bounds write) rather than a
  // fixed timeout before capturing the final bounds.
  await mapMarkers(page).first().click()
  await expect(page).toHaveURL(/[?&]station=[^&]+/)
  await expect
    .poll(
      async () => {
        const before = page.url()
        await page.waitForTimeout(250)
        return page.url() === before
      },
      { timeout: 10_000 },
    )
    .toBe(true)
  const expectedMapUrl = page.url()

  // The overview's detail link navigates in-app to the station detail page.
  await page.getByRole('complementary').getByRole('link', { name: 'Open detail page' }).click()
  await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)

  // Back to map restores the exact previous map URL (bounds + open station).
  await page.getByRole('link', { name: 'Back to map' }).click()
  await expect(page).toHaveURL(expectedMapUrl)
  await expect(mapMarkers(page).first()).toBeVisible()
})

test('clicking the map preview opens the map view at the preview bounds', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const preview = page.locator('.maplibregl-map')
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

test('the settings timeframe drives the main chart and the monthly bar chart renders', async ({
  page,
}) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const statsSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Detailed statistics' }),
  })

  // The default timeframe is "This week" (1-hour buckets).
  await expect(statsSection.getByText('1-hour buckets', { exact: true })).toBeVisible()

  // Switch to "Last 30 days" via the settings dialogue (one-click option); the
  // main chart changes (1-day buckets) and the setting lands in the URL.
  await page.getByRole('button', { name: 'Calculation settings' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Last 30 days', exact: true }).click()
  await dialog.getByRole('button', { name: 'Apply' }).click()
  await expect(statsSection.getByText('1-day buckets', { exact: true })).toBeVisible()
  await expect(page).toHaveURL(/[?&]timeframe=last_30_days/)

  // The standalone monthly bar chart renders with a year selector: one clickable
  // button per year in the header (the grand total is gone — totals are per year).
  const monthlyCard = page
    .locator('section')
    .filter({ has: page.getByRole('heading', { name: 'Bikes per month' }) })
    .locator('[data-slot="card"]')
  await expect(monthlyCard).toBeVisible()
  await expect(monthlyCard.getByRole('button').first()).toBeVisible()

  // The bar chart draws its Y axis on the left with tick labels (regression:
  // the chart used to have no Y-axis at all), and every year button shows its
  // total with the "bikes" unit. recharts 3 moved the tick labels into their
  // own z-index layer group (`recharts-yAxis-tick-labels`) and hides the
  // domain-edge tick via its tick-visibility logic, so assert any visible
  // tick label instead of the first one.
  await expect(
    monthlyCard.locator('.recharts-yAxis-tick-labels .recharts-cartesian-axis-tick-value').first(),
  ).toBeVisible()
  await expect(monthlyCard.getByRole('button').first()).toContainText('bikes')

  // The most recent year is always incomplete, so its button shows the "–"
  // marker instead of a trend vs the previous year (regression: it used to show
  // a misleading p-%).
  const latestYearButton = monthlyCard.getByRole('button').last()
  await expect(latestYearButton).toContainText('–')
  await expect(latestYearButton).not.toContainText('%')
})

test('the settings compare-previous checkbox overlays the previous period', async ({ page }) => {
  const stationId = await openFirstStation(page)
  await page.goto(`/stations/${stationId}`, { waitUntil: 'domcontentloaded' })

  const statsSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Detailed statistics' }),
  })

  await page.getByRole('button', { name: 'Calculation settings' }).click()
  const dialog = page.getByRole('dialog')
  const compare = dialog.getByRole('checkbox', { name: 'Compare previous period' })
  await expect(compare).toBeVisible()

  // Default week chart is single-series, so no "Last week" legend entry.
  await expect(statsSection.getByText('Last week', { exact: true })).toHaveCount(0)

  // Checking the box overlays the previous period. The previous period appears
  // as a "Last week" legend entry when the current week also has data; a
  // still-importing/older dataset can leave the current week empty, in which
  // case the previous period draws as the single series (and the legend is
  // intentionally hidden for a single series), so the chart must not stay
  // empty. The checkbox is controlled by the URL; `.click()` + an explicit
  // wait is more robust than `.check()` against the URL round-trip re-render.
  await compare.click()
  await expect(compare).toBeChecked()
  await dialog.getByRole('button', { name: 'Apply' }).click()
  const lastWeek = statsSection.getByText('Last week', { exact: true }).first()
  if ((await lastWeek.count()) > 0) {
    await expect(lastWeek).toBeVisible()
  } else {
    await expect(statsSection.getByText('No data for this period.', { exact: true })).toHaveCount(0)
  }
})
