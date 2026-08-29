// Temporary debug: creates a standalone MapLibre map with the app's basemap
// style, drives it to exact center/zoom, and reports (a) which tiles loaded,
// (b) queryRenderedFeatures counts for the world layers, and (c) an ASCII
// render of the map canvas. Bypasses the app's fitBounds so rendering can be
// verified under full control.
import { chromium } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const MAPLIBRE_JS = path.join(ROOT, 'node_modules/maplibre-gl/dist/maplibre-gl.js')
const STYLE_PATH = path.join(ROOT, 'public/styles/basemap.json')

const center = process.argv[2] ?? '0,0'
const zoom = Number(process.argv[3] ?? 1)
const [lng, lat] = center.split(',').map(Number)

if (!fs.existsSync(MAPLIBRE_JS)) {
  console.error('maplibre dist not found:', MAPLIBRE_JS)
  process.exit(1)
}

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1200, height: 800 } })

await page.route('**/maplibre-gl.js', (route) =>
  route.fulfill({ path: MAPLIBRE_JS, contentType: 'application/javascript' }),
)
await page.route('**/style.json', (route) =>
  route.fulfill({ path: STYLE_PATH, contentType: 'application/json' }),
)

const tileRequests = []
page.on('response', (res) => {
  if (/\/api\/map\/(world|basemap)\//.test(res.url())) tileRequests.push(`${res.status()} ${res.url()}`)
})
const consoleErrors = []
page.on('console', (m) => {
  if (m.type() === 'error' || m.type() === 'warning') consoleErrors.push(`[${m.type()}] ${m.text()}`)
})
page.on('pageerror', (e) => consoleErrors.push(`[pageerror] ${e.message}`))

await page.goto('http://localhost:8081/', { waitUntil: 'domcontentloaded' })
await page.setContent(`
<!doctype html><html><head><meta charset="utf-8">
<style>html,body{margin:0;height:100%}#m{width:100%;height:100%}</style>
</head><body><div id="m"></div>
<script src="/maplibre-gl.js"></script>
<script>
  const map = new maplibregl.Map({
    container: 'm',
    style: '/style.json',
    center: [${lng}, ${lat}],
    zoom: ${zoom},
    renderWorldCopies: false
  });
  window.__ready = new Promise((resolve) => map.on('load', resolve));
  window.__map = map;
</script></body></html>
`)
await page.waitForFunction(() => window.__ready).catch(() => {})
await page.waitForTimeout(4_000)

const result = await page.evaluate(() => {
  const map = window.__map
  const c = map.getCenter()
  const qw = map.queryRenderedFeatures({ layers: ['world-water'] })
  const ql = map.queryRenderedFeatures({ layers: ['world-land'] })
  const qbw = map.queryRenderedFeatures({ layers: ['de-water'] })
  return {
    zoom: map.getZoom(),
    center: [c.lng, c.lat],
    worldWater: qw.length,
    worldLand: ql.length,
    deWater: qbw.length,
  }
})

console.log(`\n===== standalone center=${center} zoom=${zoom} =====`)
console.log('map state:', JSON.stringify(result))
console.log('tiles loaded:')
console.log(tileRequests.length ? tileRequests.join('\n') : '(none)')
console.log('console errors/warnings:')
console.log(consoleErrors.length ? consoleErrors.join('\n') : '(none)')

// ASCII of the map canvas.
const buf = await page.locator('.maplibregl-canvas').first().screenshot()
const b64 = buf.toString('base64')
const rows = await page.evaluate(
  async ({ b64 }) => {
    const img = new Image()
    img.src = 'data:image/png;base64,' + b64
    await img.decode()
    const W = 140
    const H = 42
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
  { b64 },
)
console.log(rows.join('\n'))
await browser.close()
