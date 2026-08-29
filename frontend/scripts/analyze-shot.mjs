// Temporary debug: renders a captured map screenshot into a coarse ASCII color
// map so the world-coastline rendering can be inspected without image support.
//
//   node frontend/scripts/analyze-shot.mjs frontend/test-results/shot-world.png
//
import { chromium } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

const target = process.argv[2]
if (!target) {
  console.error('usage: node analyze-shot.mjs <path-to-png>')
  process.exit(1)
}

const W = 120
const H = 50
const abs = path.resolve(target)
const b64 = fs.readFileSync(abs).toString('base64')
const dataUrl = `data:image/png;base64,${b64}`

const browser = await chromium.launch()
const page = await browser.newPage()
await page.goto('about:blank')
await page.evaluate(async (src) => {
  const img = new Image()
  img.src = src
  await img.decode()
  window.__img = img
}, dataUrl)

const rows = await page.evaluate(
  ({ W, H }) => {
    const img = window.__img
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
        if (max < 40) c = '#' // dark
        else if (r > 210 && g > 210 && b > 210) c = '.' // near-white
        else if (b >= r && b >= g && b - max * 0.4 > 15) c = '~' // blue-dominant (water)
        else if (r > 150 && g > 150 && b < 210 && r > b && g > b) c = 'w' // beige/tan land
        else if (r > g && g >= b) c = 'o' // warm
        else if (r > 190 && g > 190 && b > 140) c = 'w'
        else c = '+'
        line += c
      }
      out.push(line)
    }
    return out
  },
  { W, H },
)

console.log(rows.join('\n'))
await browser.close()
