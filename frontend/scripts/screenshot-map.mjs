// Temporary debug: screenshots the running map at the default view and at a
// world view (via URL bounds) so the world-coastline rendering can be inspected.
import { chromium } from '@playwright/test'

const base = 'http://localhost:8081/'

async function shot(url, file) {
  const browser = await chromium.launch()
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
  await page.goto(url, { waitUntil: 'networkidle', timeout: 60_000 })
  await page.waitForTimeout(4_000) // let style + tiles load
  await page.screenshot({ path: file })
  await browser.close()
  console.log('saved', file)
}

// Default view (Münster z13) and a whole-world view via URL bounds.
await shot(base, 'frontend/test-results/shot-default.png')
await shot(
  base + '?min_lat=-60&min_lng=-180&max_lat=60&max_lng=180',
  'frontend/test-results/shot-world.png',
)
