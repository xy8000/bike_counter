import { expect, test } from '@playwright/test'

/// The global-summary request behind the header timestamp.
const SUMMARY_URL = '**/api/bff/global-summary*'

test('the header keeps only the update timestamp and opens the global summary popup', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })

  // The timestamp trigger replaces the loading skeleton once the summary loads.
  const trigger = page.getByRole('button', { name: /^updated / })
  await expect(trigger).toBeVisible()

  // The counting stats are no longer printed inline in the header.
  const header = page.locator('header').first()
  await expect(header.getByText(/\d+ stations/)).toHaveCount(0)
  await expect(header.getByText(/channels/)).toHaveCount(0)
  await expect(header.getByText(/bikes \/ last day/)).toHaveCount(0)

  const headerTimestamp = ((await trigger.textContent()) ?? '').replace(/^updated\s*/, '').trim()
  expect(headerTimestamp).not.toBe('')

  // Clicking the timestamp opens the summary popup with the full stats and a
  // repeated update timestamp, so nothing is hidden.
  await trigger.click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Global summary' })).toBeVisible()
  await expect(dialog.getByText('Counting stations', { exact: true })).toBeVisible()
  await expect(dialog.getByText('Channels', { exact: true })).toBeVisible()
  await expect(dialog.getByText('Bikes / last day', { exact: true })).toBeVisible()
  await expect(dialog.getByText('Updated', { exact: true })).toBeVisible()
  await expect(dialog.getByText(headerTimestamp, { exact: true })).toBeVisible()

  // Escape closes the popup.
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
})

test('the summary loading skeleton matches the size of the loaded timestamp trigger', async ({
  page,
}) => {
  // Hold the global-summary response so the skeleton is observable.
  let release!: () => void
  let resolveGate!: () => void
  const gate = new Promise<void>((resolve) => {
    resolveGate = resolve
  })
  release = () => resolveGate()
  await page.route(SUMMARY_URL, async (route) => {
    await gate
    await route.continue()
  })

  await page.goto('/', { waitUntil: 'domcontentloaded' })

  const header = page.locator('header').first()
  const skeleton = header.locator('[data-slot="skeleton"]')
  await expect(skeleton).toBeVisible()

  const headerBoxLoading = await header.boundingBox()
  const skeletonBox = await skeleton.boundingBox()

  release()
  const trigger = page.getByRole('button', { name: /^updated / })
  await expect(trigger).toBeVisible()
  await expect(skeleton).toHaveCount(0)

  const headerBoxLoaded = await header.boundingBox()
  const triggerBox = await trigger.boundingBox()

  // No layout shift: the app bar keeps its size and the skeleton is as high as
  // the content that replaces it (regression gate for the loading ghost).
  expect(headerBoxLoading).not.toBeNull()
  expect(headerBoxLoaded).not.toBeNull()
  expect(headerBoxLoaded?.height).toBeCloseTo(headerBoxLoading?.height ?? -1, 0)
  expect(skeletonBox).not.toBeNull()
  expect(triggerBox).not.toBeNull()
  expect(triggerBox?.height).toBeCloseTo(skeletonBox?.height ?? -1, 0)
  expect(triggerBox?.width).toBeGreaterThan(0)
})
