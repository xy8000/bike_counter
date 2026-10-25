import { expect, test } from '@playwright/test'

/// The header tag icon reveals the installed frontend and backend versions.
test('the header tag icon opens the version dialog with frontend and backend versions', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' })

  const trigger = page.getByRole('button', { name: 'Show versions' })
  await expect(trigger).toBeVisible()

  await trigger.click()

  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Versions' })).toBeVisible()

  // Each row renders its version as `vX.Y.Z` (auto-retrying while the backend
  // version request is still in flight).
  const frontendRow = dialog.locator('dl > div').filter({ hasText: 'Frontend' })
  const backendRow = dialog.locator('dl > div').filter({ hasText: 'Backend' })
  await expect(frontendRow.locator('dd')).toHaveText(/^v\d+\.\d+\.\d+$/)
  await expect(backendRow.locator('dd')).toHaveText(/^v\d+\.\d+\.\d+$/)

  // The backend value is served by /api/bff/version and must match exactly.
  const api = await page.request.get('/api/bff/version')
  expect(api.ok()).toBe(true)
  const { version } = (await api.json()) as { version: string }
  await expect(backendRow.locator('dd')).toHaveText(`v${version}`)

  // Escape closes the dialog.
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
})
