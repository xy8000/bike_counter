// Temporary debug: drives the live map, captures browser console + tile-network
// results, and lists every tile response status. Pinpoints which world tiles fail.
import { chromium } from '@playwright/test'

const base = 'http://localhost:8081/'
const urls = [
  base,
  base + '?min_lat=-60&min_lng=-180&max_lat=60&max_lng=180',
  base + '?min_lat=-80&min_lng=-170&max_lat=80&max_lng=170',
]

const browser = await chromium.launch()
for (const url of urls) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
  const consoleLines = []
  const failed = []
  const badStatus = []
  const tileResponses = []
  page.on('console', (msg) => {
    const text = msg.text()
    if (/tile|error|unable|parse|fail/i.test(text)) consoleLines.push(`[${msg.type()}] ${text}`)
  })
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`))
  page.on('requestfailed', (req) => {
    if (/\/api\/map\//.test(req.url())) failed.push(`${req.url()} -> ${req.failure()?.errorText}`)
  })
  page.on('response', (res) => {
    if (/\/api\/map\/(world|basemap)\//.test(res.url())) {
      tileResponses.push(`${res.status()} ${res.url()}`)
      if (res.status() >= 400) badStatus.push(`${res.status()} ${res.url()}`)
    }
  })

  console.log(`\n===== ${url} =====`)
  await page.goto(url, { waitUntil: 'networkidle', timeout: 60_000 })
  await page.waitForTimeout(5_000)
  console.log('--- fitted URL after load ---')
  console.log(page.url())

  console.log('--- console (tile/error lines) ---')
  console.log(consoleLines.length ? consoleLines.join('\n') : '(none)')
  console.log('--- requestfailed ---')
  console.log(failed.length ? failed.join('\n') : '(none)')
  console.log('--- bad tile status (>=400) ---')
  console.log(badStatus.length ? badStatus.join('\n') : '(none)')
  console.log(`--- tile responses: ${tileResponses.length} total ---`)
  console.log(tileResponses.join('\n'))

  await page.close()
}
await browser.close()
