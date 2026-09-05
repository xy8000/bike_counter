import { expect, test } from '@playwright/test'
import {
  cityUrl,
  mapMarkers,
  readSidebarCounts,
  sidebar,
  sidebarBadge,
  sidebarStationItems,
  waitForStations,
} from './helpers'

test('the sidebar renders only the stations visible in the current viewport', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const baseline = await readSidebarCounts(page)
  const baselineItems = await sidebarStationItems(page).count()
  const baselineMarkers = await mapMarkers(page).count()

  // The counter badge matches the number of rendered sidebar entries…
  expect(baselineItems).toBeGreaterThan(0)
  expect(baseline.visible).toBe(baselineItems)
  // …the markers match the sidebar (same viewport, same positioned set)…
  expect(baselineMarkers).toBe(baselineItems)
  // …and the badge total is the overall station count (>= visible).
  expect(baseline.total).toBeGreaterThanOrEqual(baseline.visible)

  // The shell renders an image thumbnail per row directly; the stats line
  // populates once the parallel stats sub-resource arrives (auto-waited).
  const firstRow = sidebar(page).locator('li:has(button)').first()
  await expect(firstRow.locator('img')).toBeVisible()
  await expect(firstRow.getByText('bikes / last day')).toBeVisible()

  // Focus the map and zoom in with the keyboard (the MapLibre navigation
  // control sits at the top-right, clear of the sidebar). After each moveend
  // the map re-renders and can drop a keypress, so keep zooming until the
  // visible set actually shrinks.
  const map = page.locator('.maplibregl-map')
  // The focus click must land on map "void", not a marker: a marker hit opens
  // the station overview, which replaces the sidebar and breaks the count
  // assertions. Click a far corner, well clear of the central Münster cluster.
  await map.click({ position: { x: 1200, y: 620 } })
  // Guard: if the click still opened the overview, close it so the sidebar
  // badge is readable again before the zoom poll.
  const closeOverview = page.getByRole('button', { name: 'Close station overview' })
  if (await closeOverview.isVisible().catch(() => false)) {
    await closeOverview.click()
  }
  await map.focus()
  await expect
    .poll(
      async () => {
        const current = (await readSidebarCounts(page)).visible
        if (current >= baseline.visible) {
          await page.keyboard.press('+')
          await page.waitForTimeout(400)
        }
        return (await readSidebarCounts(page)).visible
      },
      { timeout: 20000 },
    )
    .toBeLessThan(baseline.visible)

  // After zooming, the visible set shrank and stays internally consistent.
  // The map settles asynchronously after the last zoom step (markers and the
  // sidebar re-render on moveend), so wait for them to agree with the visible
  // counter instead of asserting a single, possibly transient read.
  await expect
    .poll(
      async () => {
        const counts = await readSidebarCounts(page)
        const items = await sidebarStationItems(page).count()
        const markers = await mapMarkers(page).count()
        return items === counts.visible && markers === counts.visible
      },
      { timeout: 20000 },
    )
    .toBe(true)
  const after = await readSidebarCounts(page)
  expect(after.visible).toBeLessThan(baseline.visible)
})

test('a collapsed panel re-opens onto the overview when a station is selected', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // Collapse the station list first.
  await page.getByRole('button', { name: 'Hide station list' }).click()
  await expect(page.getByRole('button', { name: 'Show station list' })).toBeVisible()

  // Opening a station's overview while collapsed keeps the panel collapsed…
  await mapMarkers(page).first().click()
  await expect(page.getByRole('button', { name: 'Show station list' })).toBeVisible()

  // …and pulling it open reveals the overview, not the station list.
  await page.getByRole('button', { name: 'Show station list' }).click()
  const panel = page.getByRole('complementary')
  await expect(panel.getByText('Total bikes (all time)')).toBeVisible()
  await expect(panel.getByText('Visible counting stations')).toHaveCount(0)
})

test('the mid-height pull/push handle slides the sidebar in and out', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // Expanded: the "push" handle is on the panel's right edge, mid-height.
  const push = page.getByRole('button', { name: 'Hide station list' })
  await expect(push).toBeVisible()
  await expect(sidebarBadge(page)).toBeVisible()

  // Pushing slides the whole panel left off-screen (a negative CSS `translate`);
  // only the "pull" handle stays visible at the map's left edge.
  await push.click()
  const pull = page.getByRole('button', { name: 'Show station list' })
  await expect(pull).toBeVisible()
  await expect(sidebar(page)).toHaveCSS('translate', /-/)

  // Pulling slides it back out over the map.
  await pull.click()
  await expect(page.getByRole('button', { name: 'Hide station list' })).toBeVisible()
  await expect(sidebarBadge(page)).toBeVisible()
})

test('the sidebar lists stations by last-day count, busiest first', async ({ page }) => {
  // The Münster city view exposes the whole seeded Münster set. Only four of
  // those stations get synthesized measurements (see e2e-seed.sql): Gasselstiege
  // has six channels with data, so it is by far the busiest and must top the
  // list even though its name sorts after Bismarckallee / Bohlweg / Coesfelder
  // Kreuz alphabetically.
  await page.goto(cityUrl('Münster'), { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const rows = sidebarStationItems(page)
  // The stats sub-resource arrives after the shell; wait until a real count
  // replaces the per-row stats skeleton before reading the order.
  await expect(rows.first().getByText('bikes / last day')).toBeVisible()

  // Read each row's station name and its formatted last-day count. Counts use
  // the de-DE thousands separator, so strip '.'/',' to recover the integer.
  const entries = await rows.evaluateAll((items) =>
    items.map((item) => {
      const name =
        (item.querySelector('span.font-semibold') as HTMLElement | null)?.textContent?.trim() ?? ''
      const label = item.textContent ?? ''
      const raw = label.match(/([\d.,]+)\s+bikes \/ last day/)?.[1] ?? '0'
      return { name, bikes: Number(raw.replace(/[.,]/g, '')) }
    }),
  )
  expect(entries.length).toBeGreaterThan(1)

  // Busiest first: the last-day counts are non-increasing down the list…
  for (let index = 1; index < entries.length; index += 1) {
    expect(entries[index].bikes).toBeLessThanOrEqual(entries[index - 1].bikes)
  }
  // …and Gasselstiege (6 channels with data) leads a list that is not the
  // alphabetical order of the seeded Münster stations.
  expect(entries[0].name).toBe('Gasselstiege')
})
