//! Generates the coarse world vector-tile backdrop for the self-hosted map:
//!
//!   tiles/world.mbtiles   Natural Earth whole-world context (zoom 0-5) so the
//!                         map is never blank when zoomed out: an ocean
//!                         (`water`) layer plus land polygons (`landcover`)
//!                         clipped from the Natural Earth 50m land data (via
//!                         the `world-atlas` devDependency). Real coastlines,
//!                         no OSM detail. (50m — not 110m — so the coastlines
//!                         are visibly finer when zoomed out.)
//!
//! The `martin` docker service serves this file as the `world` source
//! (`/world/{z}/{x}/{y}`); the Germany OSM detail comes from
//! `tiles/basemap.pmtiles` (built once by `scripts/build_tiles.sh`).
//!
//! This is a one-time data-provisioning step (not pre-rendering in the app
//! pipeline): it is tiny (~2 MB, z0-5 = 1,365 tiles) and fast, so `make tiles`
//! / `make run` build it on demand, and CI/e2e regenerate it so the tile-proxy
//! assertion has real data.
//!
//!   node frontend/scripts/build-world-tiles.mjs
//!
//! Uses `vt-pbf` (MVT writer), `world-atlas` (Natural Earth land) and
//! `topojson-client` (TopoJSON -> GeoJSON) plus Node's built-in `node:sqlite`
//! (Node >= 22.5).

import { existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { DatabaseSync } from 'node:sqlite'
import vtpbf from 'vt-pbf'
import { feature as topojsonFeature } from 'topojson-client'

const require = createRequire(import.meta.url)

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const TILES_DIR = path.join(REPO_ROOT, 'tiles')
const WORLD_OUT = path.join(TILES_DIR, 'world.mbtiles')
const WORLD_MAX_ZOOM = 5

const EXTENT = 4096
const MAX_LAT = 85.0511287798066 // Web Mercator latitude limit

// --- MVT building (via vt-pbf) ---

function feature(type, geometry) {
  return {
    type, // 2 = LineString, 3 = Polygon
    properties: {},
    loadGeometry() {
      return geometry
    },
  }
}

function layer(name, features) {
  return {
    name,
    version: 2,
    extent: EXTENT,
    length: features.length,
    feature(i) {
      return features[i]
    },
  }
}

function encodeMvt(layers) {
  return new Uint8Array(vtpbf({ layers }))
}

const FULL_EXTENT_RING = [
  { x: 0, y: 0 },
  { x: 0, y: EXTENT },
  { x: EXTENT, y: EXTENT },
  { x: EXTENT, y: 0 },
  { x: 0, y: 0 },
]

// --- World land (Natural Earth 50m via world-atlas) ---

const LAND_TOPOLOGY = require('world-atlas/land-50m.json')
const landFeatures = topojsonFeature(LAND_TOPOLOGY, LAND_TOPOLOGY.objects.land).features

/// Every land polygon as `{ outer, holes, bbox }` in [lon, lat] degrees; the
/// cached bbox keeps per-tile culling cheap.
const worldPolygons = []
for (const landFeature of landFeatures) {
  const geometry = landFeature.geometry
  const polys = geometry.type === 'Polygon' ? [geometry.coordinates] : geometry.coordinates
  for (const poly of polys) {
    const outer = poly[0]
    const holes = poly.slice(1)
    let minLon = Infinity
    let maxLon = -Infinity
    let minLat = Infinity
    let maxLat = -Infinity
    for (const [lon, lat] of outer) {
      if (lon < minLon) minLon = lon
      if (lon > maxLon) maxLon = lon
      if (lat < minLat) minLat = lat
      if (lat > maxLat) maxLat = lat
    }
    worldPolygons.push({ outer, holes, bbox: { minLon, maxLon, minLat, maxLat } })
  }
}

// --- Web Mercator projection + tile clipping ---

function clampLat(lat) {
  return Math.max(-MAX_LAT, Math.min(MAX_LAT, lat))
}

function projectPoint(z, x, y, lon, lat) {
  const size = 2 ** z * EXTENT
  const latRad = (clampLat(lat) * Math.PI) / 180
  return {
    x: ((lon + 180) / 360) * size - x * EXTENT,
    y: ((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2) * size - y * EXTENT,
  }
}

function latForY(z, y) {
  const n = 2 ** z
  return (Math.atan(Math.sinh(Math.PI * (1 - (2 * y) / n))) * 180) / Math.PI
}

function intersectLon(a, b, lon) {
  const t = (lon - a[0]) / (b[0] - a[0])
  return [lon, a[1] + t * (b[1] - a[1])]
}

function intersectLat(a, b, lat) {
  const t = (lat - a[1]) / (b[1] - a[1])
  return [a[0] + t * (b[0] - a[0]), lat]
}

/// Sutherland-Hodgman clip of a ring (array of [lon, lat]) against the tile
/// bounds; returns the clipped ring or null when nothing remains.
function clipRing(ring, w, e, s, n) {
  const edges = [
    { inside: (p) => p[0] >= w, isect: (a, b) => intersectLon(a, b, w) },
    { inside: (p) => p[0] <= e, isect: (a, b) => intersectLon(a, b, e) },
    { inside: (p) => p[1] >= s, isect: (a, b) => intersectLat(a, b, s) },
    { inside: (p) => p[1] <= n, isect: (a, b) => intersectLat(a, b, n) },
  ]
  let poly = ring.map((p) => [p[0], p[1]])
  for (const edge of edges) {
    const next = []
    for (let i = 0; i < poly.length; i++) {
      const a = poly[i]
      const b = poly[(i + 1) % poly.length]
      const aIn = edge.inside(a)
      const bIn = edge.inside(b)
      if (aIn) next.push(a)
      if (aIn !== bIn) next.push(edge.isect(a, b))
    }
    poly = next
    if (poly.length === 0) return null
  }
  return poly.length >= 3 ? poly : null
}

function ringToPixel(z, x, y, ring) {
  return ring.map(([lon, lat]) => {
    const p = projectPoint(z, x, y, lon, lat)
    return { x: p.x, y: p.y }
  })
}

/// A world tile: an ocean `water` layer (full extent) plus the land polygons
/// (`landcover`) clipped to the tile. Tiles without land are ocean-only.
function worldMvt(z, x, y) {
  const n = 2 ** z
  const w = (x / n) * 360 - 180
  const e = ((x + 1) / n) * 360 - 180
  const south = latForY(z, y + 1)
  const north = latForY(z, y)

  const landPolygons = []
  for (const poly of worldPolygons) {
    const { bbox } = poly
    if (bbox.maxLon < w || bbox.minLon > e || bbox.maxLat < south || bbox.minLat > north) continue
    const outer = clipRing(poly.outer, w, e, south, north)
    if (!outer) continue
    const holes = []
    for (const hole of poly.holes) {
      const clipped = clipRing(hole, w, e, south, north)
      if (clipped) holes.push(ringToPixel(z, x, y, clipped))
    }
    landPolygons.push([ringToPixel(z, x, y, outer), ...holes])
  }

  return encodeMvt({
    water: layer('water', [feature(3, [FULL_EXTENT_RING])]),
    landcover: layer('landcover', landPolygons.map((poly) => feature(3, poly))),
  })
}

// --- MBTiles writer ---

function createMbtilesWriter(file, metaRows) {
  mkdirSync(TILES_DIR, { recursive: true })
  rmSync(file, { force: true })
  const db = new DatabaseSync(file)
  db.exec(`
    CREATE TABLE metadata (name TEXT, value TEXT);
    CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
    CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
  `)
  const insertMeta = db.prepare('INSERT INTO metadata (name, value) VALUES (?, ?)')
  for (const [name, value] of metaRows) insertMeta.run(name, value)
  const insertTile = db.prepare(
    'INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?, ?, ?, ?)',
  )
  db.exec('BEGIN')
  let count = 0
  return {
    add(t) {
      insertTile.run(t.z, t.x, 2 ** t.z - 1 - t.y, Buffer.from(t.data))
      count++
    },
    finish() {
      db.exec('COMMIT')
      db.close()
      return count
    },
  }
}

function main() {
  const worldVectorLayers = JSON.stringify([{ id: 'water' }, { id: 'landcover' }])
  const writer = createMbtilesWriter(WORLD_OUT, [
    ['name', 'world'],
    ['format', 'pbf'],
    ['type', 'overlay'],
    ['minzoom', '0'],
    ['maxzoom', String(WORLD_MAX_ZOOM)],
    ['bounds', '-180,-85.0511287798066,180,85.0511287798066'],
    ['center', '0,0,2'],
    ['vector_layers', worldVectorLayers],
    ['json', JSON.stringify({ format: 'pbf', vector_layers: JSON.parse(worldVectorLayers) })],
  ])
  for (let z = 0; z <= WORLD_MAX_ZOOM; z++) {
    const n = 2 ** z
    for (let x = 0; x < n; x++) {
      for (let y = 0; y < n; y++) {
        writer.add({ z, x, y, data: worldMvt(z, x, y) })
      }
    }
  }
  writer.finish()

  // Self-verify by reopening the archive.
  const check = new DatabaseSync(WORLD_OUT)
  const count = check.prepare('SELECT COUNT(*) AS c FROM tiles').get()
  const europe = check
    .prepare('SELECT length(tile_data) AS len FROM tiles WHERE zoom_level = 2 AND tile_column = 2 AND tile_row = ?')
    .get(2 ** 2 - 1 - 1) // z2 tile (2,1) covers Europe/Africa (TMS row = 2^2-1-1)
  check.close()
  if (!europe || !europe.len) {
    throw new Error('self-check failed: Europe world tile missing from the archive')
  }

  console.log(
    `generated ${WORLD_OUT}: ${count.c} tiles (z0-${WORLD_MAX_ZOOM}), ` +
      `${readFileSync(WORLD_OUT).length} bytes, Europe z2 tile ${europe.len} bytes`,
  )
}

try {
  main()
} catch (error) {
  console.error(`build-world-tiles: ${error.stack ?? error}`)
  process.exit(1)
}
