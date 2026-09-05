import { expect, test, type Page } from '@playwright/test'
import { mapMarkers, sidebar, waitForStations } from './helpers'

const SUMMARIZE_BUTTON = 'Summarize visible stations'

/// The seeded stations whose channels carry synthesized measurements (see
/// scripts/dump-e2e-fixture.sh). The summary key-facts and charts derive from a
/// station's data, so the summary URL must scope to one of these — the search
/// covers all seven sources, including data-less Hessen/Köln/Eco-Counter
/// stations that sort first.
const DATA_STATION_NAMES = new Set([
  'Bismarckallee',
  'Bohlweg',
  'Coesfelder Kreuz',
  'Gasselstiege',
  'BN - Bröltalbahnweg',
  'BN - Brühler Straße',
  'BN - Estermannufer',
  'MQ1.2',
  'MQ1.3',
  'MQ10.1+10.2',
])

/// A summary URL around one data-bearing station. The real map view can
/// aggregate the whole cluster (which is heavy and intentionally shows the
/// loading page), so the tests scope the summary to one station to keep the
/// browser fast and deterministic while still exercising the full feature.
async function smallSummaryUrl(page: Page): Promise<string> {
  const response = await page.request.get('/api/bff/stations/search')
  const data = await response.json()
  const positioned = (data.items ?? []).filter(
    (s: { latitude?: number | null; longitude?: number | null }) =>
      s.latitude != null && s.longitude != null,
  )
  const station =
    positioned.find((s: { name: string }) => DATA_STATION_NAMES.has(s.name)) ?? positioned[0]
  if (!station) throw new Error('no positioned station for the summary e2e')
  const span = 0.002
  const params = new URLSearchParams({
    min_lat: String(station.latitude - span),
    min_lng: String(station.longitude - span),
    max_lat: String(station.latitude + span),
    max_lng: String(station.longitude + span),
  })
  return `/summary?${params.toString()}`
}

/// Wait for the page shell to render (the station list + title arrive first;
/// the aggregated stats cards then load their own sub-resources, which
/// Playwright auto-waits on in the assertions below).
async function waitForSummaryContent(page: Page) {
  await expect(page.getByRole('heading', { name: 'Station summary' })).toBeVisible({
    timeout: 240_000,
  })
}

test.describe('station summary', () => {
  test.describe.configure({ timeout: 300_000 })

  test('the sidebar opens the shareable station-summary page at the current view', async ({
    page,
  }) => {
    await page.goto('/', { waitUntil: 'domcontentloaded' })
    await waitForStations(page)

    // The pinned sidebar footer holds the summarize action (the list above stays
    // scrollable), enabled as soon as there are visible stations.
    const button = sidebar(page).getByRole('button', { name: SUMMARIZE_BUTTON })
    await expect(button).toBeVisible()
    await expect(button).toBeEnabled()
    await button.click()

    // The summary page opens with the map-view bounds in the URL (the same view
    // the user selected) and a back link.
    await expect(page).toHaveURL(/\/summary\?.*min_lat=/)
    await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
  })

  test('the summary page renders the fallback image, map and aggregated sections', async ({
    page,
  }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })

    await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
    await expect(page.getByRole('img', { name: 'Station summary image' })).toBeVisible()
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()

    // The aggregated page sections, including the all-time counter.
    await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible()
    await expect(page.getByText('Total bikes (all time)')).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Detailed statistics' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Detailed stats' })).toBeVisible()
    // The timeframe-derived key facts render inside "Detailed statistics".
    await expect(
      page
        .locator('section')
        .filter({ has: page.getByRole('heading', { name: 'Detailed statistics' }) })
        .getByText('Total bikes in selection', { exact: true }),
    ).toBeVisible()
    // The hour-of-day radar sits next to the Weekdays radar (split half).
    await expect(
      page.locator('[data-slot="card"]').filter({ hasText: 'Hours' }).first(),
    ).toBeVisible()
    const monthlySection = page.locator('section').filter({
      has: page.getByRole('heading', { name: 'Bikes per month' }),
    })
    await expect(monthlySection).toBeVisible()
    await expect(monthlySection.locator('[data-slot="card"]')).toBeVisible()
    // Recharts draws at least one chart.
    await expect(page.locator('.recharts-wrapper').first()).toBeVisible()
  })

  test('clicking a map flag disables the station and updates the URL', async ({ page }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()

    const marker = mapMarkers(page).first()
    await marker.click()

    // The disabled station id lands in the URL (replace) …
    await expect(page).toHaveURL(/[?&]disabled=[0-9a-f-]+/)
    // … and the clicked marker turns gray once the aggregation re-runs without
    // that station: both the class and the actual computed gray filter.
    await expect(marker).toHaveClass(/station-marker--disabled/, { timeout: 240_000 })
    await expect(marker).toHaveCSS('filter', /grayscale/)
  })

  test('a shared summary URL restores the disabled station as grayed out', async ({ page }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()

    await mapMarkers(page).first().click()
    await expect(page).toHaveURL(/[?&]disabled=[0-9a-f-]+/)
    const sharedUrl = page.url()

    // A fresh page on the shared URL shows the disabled marker grayed.
    await page.goto(sharedUrl, { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)
    await expect(mapMarkers(page).first()).toBeVisible()
    const disabledMarker = page.locator('.station-marker--disabled').first()
    await expect(disabledMarker).toBeVisible({ timeout: 240_000 })
    await expect(disabledMarker).toHaveCSS('filter', /grayscale/)
  })

  test('the back-to-map link returns to the same map view', async ({ page }) => {
    await page.goto(await smallSummaryUrl(page), { waitUntil: 'domcontentloaded' })
    // The loaded page gives us the origin to resolve the relative summary path.
    const summaryUrl = new URL(page.url())

    await page.getByRole('link', { name: 'Back to map' }).click()
    // The restored map view carries the same bbox as the /summary URL it came
    // from (regression: it used to reset to the Münster default view).
    await expect(page).toHaveURL(/[?&]min_lat=/)
    const mapUrl = new URL(page.url())
    for (const key of ['min_lat', 'min_lng', 'max_lat', 'max_lng']) {
      expect(Number(mapUrl.searchParams.get(key))).toBeCloseTo(
        Number(summaryUrl.searchParams.get(key)),
        6,
      )
    }
    await expect(mapMarkers(page).first()).toBeVisible()
    await expect(sidebar(page)).toBeVisible()
  })

  test('the per-station nerd-stats charts show the info note for a view with more than 5 stations', async ({
    page,
  }) => {
    // A bounds spanning Münster + Bonn covers the 7 stations the fixture
    // synthesizes data for (4 Münster + 3 Bonn; Hamburg stays outside), so every
    // per-station chart exceeds the 5-stream limit and must show the info note.
    // (Köln/Eco-Counter stations also fall in these bounds, but they have no
    // synthesized data and therefore add no data-streams.)
    const params = new URLSearchParams({
      min_lat: '50.5',
      min_lng: '6.9',
      max_lat: '52.1',
      max_lng: '7.8',
    })
    await page.goto(`/summary?${params.toString()}`, { waitUntil: 'domcontentloaded' })
    await waitForSummaryContent(page)

    // The aggregate "Detailed statistics" section is unaffected and still draws.
    const statsSection = page.locator('section').filter({
      has: page.getByRole('heading', { name: 'Detailed statistics' }),
    })
    await expect(statsSection.locator('.recharts-wrapper').first()).toBeVisible()

    // The per-station detailed-stats charts are replaced by the info note.
    const detailedSection = page.locator('section').filter({
      has: page.getByRole('heading', { name: 'Detailed stats' }),
    })
    await expect(detailedSection.getByText(/too many data-streams to render/).first()).toBeVisible()
  })
})
