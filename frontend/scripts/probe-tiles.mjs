// Temporary probe: decodes the served world/basemap MVT tiles and summarizes
// the layer geometry, to diagnose the world-coastline rendering issue.
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const { VectorTile } = require('@mapbox/vector-tile')
// The top-level `pbf` is v5 (PbfReader/PbfWriter, no `Pbf` constructor);
// @mapbox/vector-tile needs the classic pbf v3 that vt-pbf vendors.
const Pbf = require('../node_modules/vt-pbf/node_modules/pbf')

const base = 'http://localhost:8081/api/map'

async function probe(source, z, x, y) {
  const res = await fetch(`${base}/${source}/${z}/${x}/${y}`)
  const buf = new Uint8Array(await res.arrayBuffer())
  let tile
  try {
    tile = new VectorTile(new Pbf(buf))
  } catch (e) {
    console.log(`${source}/${z}/${x}/${y}: ${res.status} DECODE FAILED: ${e.message}`)
    return
  }
  const layerNames = Object.keys(tile.layers)
  console.log(`${source}/${z}/${x}/${y}: status=${res.status} bytes=${buf.length} layers=[${layerNames.join(', ')}]`)
  for (const name of layerNames) {
    const layer = tile.layers[name]
    console.log(`  layer "${name}": ${layer.length} features`)
    for (let i = 0; i < Math.min(layer.length, 4); i++) {
      const f = layer.feature(i)
      const geom = f.loadGeometry()
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
      let pts = 0
      for (const ring of geom) {
        pts += ring.length
        for (const pt of ring) {
          minX = Math.min(minX, pt.x); minY = Math.min(minY, pt.y)
          maxX = Math.max(maxX, pt.x); maxY = Math.max(maxY, pt.y)
        }
      }
      console.log(`    f[${i}] type=${f.type} geomBBox=(${minX.toFixed(0)},${minY.toFixed(0)})-(${maxX.toFixed(0)},${maxY.toFixed(0)}) rings=${geom.length} points=${pts}`)
    }
  }
}

await probe('world', 0, 0, 0)
await probe('world', 1, 0, 0)
await probe('world', 1, 0, 1)
await probe('world', 1, 1, 0)
await probe('world', 1, 1, 1)
await probe('world', 2, 2, 1)
