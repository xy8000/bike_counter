// Temporary debug: drives the live app to a specific geographic view (via URL
// bounds) and renders a high-resolution ASCII colour map of the MAP canvas only,
// to spot blank-tile patches / broken coastlines.
import { chromium } from '@playwright/test'
import fs from 'node:fs'

const base = 'http://localhost:8081/'

// Scenarios: [name, URL-bounds query]
const scenarios = [['world0', '?min_lat=-80&min_lng=-170&max_lat=80&max_lng=170']]

const W = 200
const H = 64

const browser = await chromium.launch()
for (const [name, q] of scenarios) {
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } })
  await page.goto(base + q, { waitUntil: 'networkidle', timeout: 60_000 })
  await page.waitForTimeout(5_000)

  // Locate the map canvas element and screenshot just it.
  const mapBox = await page
    .locator('.maplibregl-canvas')
    .first()
    .boundingBox()
    .catch(() => null)
  if (!mapBox) {
    console.log(`\n===== ${name}: no map canvas found =====`)
    await page.close()
    continue
  }
  const buf = await page.screenshot({ clip: mapBox })
  fs.writeFileSync(`frontend/test-results/render-${name}.png`, buf)

  const b64 = buf.toString('base64')
  const rows = await page.evaluate(
    async ({ b64, W, H }) => {
      const img = new Image()
      img.src = 'data:image/png;base64,' + b64
      await img.decode()
      const canvas = document.createElement('canvas')
      canvas.width = W
      canvas.height = H
      const ctx = canvas.getContext('2d')
      ctx.drawImage(img, 0, 0, W, H)
      const data = ctx.getImageData(0, 0, W, H).data
      const out = []
      for (let y = 0; y < H; y++) {
        let line = ''
        for (let x = 0; x < W; x++) {
          const i = (y * W + x) * 4
          const r = data[i]
          const g = data[i + 1]
          const b = data[i + 2]
          const max = Math.max(r, g, b)
          let c
          if (max < 40) c = '#'
          else if (r > 210 && g > 210 && b > 210) c = '.'
          else if (b >= r && b >= g && b - max * 0.4 > 15) c = '~'
          else if (r > g && g >= b) c = 'o'
          else c = 'w'
          line += c
        }
        out.push(line)
      }
      return out
    },
    { b64, W, H },
  )

  console.log(
    `\n===== ${name} (map canvas ${Math.round(mapBox.width)}x${Math.round(mapBox.height)}) =====`,
  )
  console.log(rows.join('\n'))
  await page.close()
}
await browser.close()
