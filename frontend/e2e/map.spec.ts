import { expect, test } from '@playwright/test'
import { cityUrl, mapClusters, mapMarkers, sidebarBadge, waitForStations } from './helpers'

test('the map loads the self-hosted PMTiles basemap archive as a static file', async ({ page }) => {
  // MapLibre reads vector tiles directly out of the static archive via HTTP
  // range requests (the `pmtiles` protocol, see frontend/src/lib/map.tsx); no
  // BFF proxy or tile-server process is involved. Assert the archive nginx
  // serves at /tiles/map.pmtiles (see tiles/README.md) is reachable and has
  // the PMTiles magic header.
  const archive = await page.request.get('/tiles/map.pmtiles', {
    headers: { Range: 'bytes=0-15' },
  })
  expect([200, 206]).toContain(archive.status())
  expect((await archive.body()).length).toBeGreaterThan(0)
})

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

  const popup = page.locator('.station-popup')
  await expect(popup).toBeVisible()
  await expect(popup).toContainText(stationName)
  // The icon button is the explicit detail affordance; the name is also a link.
  await expect(popup.getByRole('link', { name: 'Open detail page' })).toHaveAttribute(
    'href',
    /^\/stations\//,
  )
  await expect(popup.getByRole('link', { name: stationName })).toHaveAttribute(
    'href',
    /^\/stations\//,
  )
  // The popup shows the station image icon and the channel count.
  await expect(popup.locator('img')).toBeVisible()
  await expect(popup.getByText(/\d+ channels?/)).toBeVisible()

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

  // The overview panel replaces the sidebar: it shows the image, the all-time
  // counter, the key-fact metrics with trend labels and the detail-page link.
  const overview = page.getByRole('complementary')
  await expect(overview.getByText('Total bikes (all time)')).toBeVisible()
  await expect(overview.getByText('Last 24 hours')).toBeVisible()
  await expect(overview.getByText('Last 7 days')).toBeVisible()
  await expect(overview.getByText('Last month')).toBeVisible()
  await expect(overview.getByRole('link', { name: 'Open detail page' })).toHaveAttribute(
    'href',
    /^\/stations\//,
  )
  // The heading is clickable (to the same detail page) but not styled as a link.
  await expect(overview.getByRole('link', { name: stationName })).toHaveAttribute(
    'href',
    /^\/stations\//,
  )
  // The large station image (the banner also carries a small icon thumbnail).
  await expect(overview.getByAltText(`${stationName} image`)).toBeVisible()
  // The old sidebar counter is gone while the overview is open.
  await expect(sidebarBadge(page)).toHaveCount(0)

  // Clicking the map void (far right edge, away from any marker) closes the
  // overview and restores the sidebar.
  const map = page.locator('.maplibregl-map')
  const box = await map.boundingBox()
  expect(box).not.toBeNull()
  await map.click({ position: { x: (box?.width ?? 100) - 20, y: (box?.height ?? 100) / 2 } })

  await expect(sidebarBadge(page)).toBeVisible()
})

test('the sidebar collapse also works while the overview is open', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  await mapMarkers(page).first().click()
  const panel = page.getByRole('complementary')
  await expect(panel.getByText('Total bikes (all time)')).toBeVisible()

  // The handle stays available while the overview is open: pushing collapses
  // the whole panel (overview included)…
  await page.getByRole('button', { name: 'Hide station list' }).click()
  await expect(page.getByRole('button', { name: 'Show station list' })).toBeVisible()
  await expect(panel).toHaveCSS('translate', /-/)

  // …and pulling re-opens the same overview.
  await page.getByRole('button', { name: 'Show station list' }).click()
  await expect(panel.getByText('Total bikes (all time)')).toBeVisible()
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

test('the overview footer "Open detailed view" button opens the detail page in the same tab', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const marker = mapMarkers(page).first()
  expect((await marker.getAttribute('alt')) ?? '').not.toBe('')
  await marker.click()

  // The pinned footer button is a full-width detail affordance next to the
  // header icon button; both point at the detail page.
  const overview = page.getByRole('complementary')
  const footerButton = overview.getByRole('link', { name: 'Open detailed view' })
  await expect(footerButton).toBeVisible()
  await expect(footerButton).toHaveAttribute('href', /^\/stations\//)

  // Clicking the footer button navigates in the same tab (no new page).
  await footerButton.click()
  await expect(page).toHaveURL(/\/stations\/[0-9a-f-]+/)
  await expect(page.context().pages()).toHaveLength(1)
  await expect(page.getByRole('link', { name: 'Back to map' })).toBeVisible()
})

test('the overview banner shows the name, description and channel count', async ({ page }) => {
  // A Hamburg station whose seed row carries a description (the Münster rows
  // have none), so every banner field is assertable.
  await page.goto(cityUrl('Hamburg'), { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  const marker = page.getByAltText('MQ10.1+10.2')
  await expect(marker).toBeVisible()
  await marker.click()

  const overview = page.getByRole('complementary')
  await expect(overview.getByText('MQ10.1+10.2', { exact: true })).toBeVisible()
  await expect(overview.getByText('Messquerschnitt (Zählfeld-Gruppe) MQ10.1+10.2')).toBeVisible()
  await expect(overview.getByText(/\d+ channels?/)).toBeVisible()
})

test('overlapping stations group into a numbered circle that un-groups when clicked', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await waitForStations(page)

  // The expanded sidebar overlays the left edge; collapse it so the circle is
  // clickable. This does not change the map bounds (the map is full width).
  await page.getByRole('button', { name: 'Hide station list' }).click()

  // Grouping: at the default Münster view at least one pair of close stations
  // (e.g. Hafen-/Hammer Straße) is folded into a numbered circle instead of two
  // stacked flags. The circle carries the group size as text and `data-count`.
  const cluster = mapClusters(page).first()
  await expect(cluster).toBeVisible()
  const count = Number(await cluster.getAttribute('data-count'))
  expect(count).toBeGreaterThan(1)
  await expect(cluster).toHaveText(String(count))

  const clustersBefore = await mapClusters(page).count()

  // Clicking the circle eases the map to the zoom where the cluster splits: the
  // circle disappears and the stations it stood for re-render as individual
  // flags (the map re-fetches the smaller viewport, so poll for the settle).
  await cluster.click()
  await expect.poll(async () => mapClusters(page).count()).toBeLessThan(clustersBefore)
  await expect(mapMarkers(page).first()).toBeVisible()
})
