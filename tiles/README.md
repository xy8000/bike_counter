# Self-hosted vector basemap tiles

This directory holds the **data** for the self-hosted vector basemap. The files
here are git-ignored because they are large and built/downloaded once; the
`martin` docker-compose service mounts this folder read-only at `/data` and
serves every present archive as a separate source (the source id is the file
stem, e.g. `world` from `world.mbtiles`):

- `world.mbtiles` — a coarse **global** basemap (zoom 0–5, ~1,365 tiles / ~2 MB)
  so the map is never blank when zoomed out: an ocean layer plus land polygons
  from Natural Earth (real coastlines, no OSM detail). Generated on demand by
  `node frontend/scripts/build-world-tiles.mjs` (`make tiles`) and served at
  `/world/{z}/{x}/{y}`.
- `basemap.pmtiles` — the **detailed** Germany basemap (OpenMapTiles schema,
  zoom 5–14), built from a Geofabrik Germany extract. It naturally covers the
  integrated cities (Münster, Bonn, Hamburg) at street zoom (z12–14). Served at
  `/basemap/{z}/{x}/{y}`. Built automatically by the one-shot `basemap` init
  container on `docker compose up` (or manually via `./scripts/build_tiles.sh`).

The frontend style
([`frontend/public/styles/basemap.json`](../frontend/public/styles/basemap.json))
paints the world water/land first (z0–5) and the Germany layers on top (z5–14),
so zooming out shows the whole world and zooming into Germany shows real OSM
detail.

Martin does **not** render a basemap — it serves tiles from data you provide.
Until a tile archive exists, the `martin` container stays up and logs a single
"waiting for tile data" message (it does not crash-loop); the BFF map-tile proxy
answers `502` and the rest of the stack is unaffected. The map markers/popups
render independently of the basemap.

## World backdrop (fast, no OSM data)

```bash
make tiles   # node frontend/scripts/build-world-tiles.mjs -> tiles/world.mbtiles
```

`make run` depends on `tiles`, so a clean checkout / stack reset always has the
tiny world backdrop.

## Germany OSM basemap (init container — default)

`docker compose up` runs a one-shot `basemap` init container (like `minio-init`)
that builds `tiles/basemap.pmtiles` with Planetiler into the shared `./tiles`
volume; `martin` waits for it to finish before starting. Java runs **only inside
that container** — the host needs no Java/Rust toolchain. The build is skipped
when `basemap.pmtiles` already exists (cached across runs), and `SKIP_BASEMAP=1`
skips it entirely (the e2e uses this — it only needs the world backdrop).

On a 14 GB machine the init container uses Planetiler's low-RAM profile
(`--storage=mmap --nodemap-type=sparse`, `-Xmx8g`), which fits alongside the
running stack. Override via the `JAVA_MEM` / `STORAGE` / `NODEMAP_TYPE`
environment variables. The first run downloads the ~3.5 GB Geofabrik extract and
takes a while.

Alternatively, drop any pre-built OpenMapTiles/OpenFreeMap Germany `.pmtiles` at
`tiles/basemap.pmtiles`, or build on the host with
`./scripts/build_tiles.sh`. Martin serves `basemap.pmtiles` if present; the
world `world.mbtiles` remains the zoomed-out backdrop.

## Updating the basemap (new streets/roads)

The basemap is a **static snapshot** of OSM at build time — it does not update
on its own (the station/measurement data does, via the data-source job). There is
no incremental tile update: vector-tile builders process the full extract, so
refreshing means rebuilding from the latest extract:

```bash
make basemap-update   # drops the cached pmtiles + Germany extract, re-runs the
                      # basemap init container (re-downloads the latest extract +
                      # rebuilds, slow), then restarts martin
```

## After provisioning

```bash
docker compose up -d --force-recreate martin
```

Then verify a tile comes back through the whole chain for each source:

```bash
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' \
  http://localhost:8081/api/map/world/2/2/1          # Europe (world source)
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' \
  http://localhost:8081/api/map/basemap/13/4269/2707 # Münster (basemap source)
```

Both should print `200 application/x-protobuf`.

## Notes

- The frontend uses `maplibre-gl@^6`. maplibre-gl 6 parses tiles in a separate
  Web Worker (`maplibre-gl-worker.mjs`, which imports
  `./maplibre-gl-shared.mjs`); the bundler does not emit it on its own, so
  [`lib/map.tsx`](../frontend/src/lib/map.tsx:1) bundles it with Vite
  (`?worker&url`) and registers it via `setWorkerUrl()` before any map is
  created — without this, no tiles load.
- `BaseMap` fetches the style at runtime and rewrites each vector source's
  `tiles` to absolute URLs, because maplibre-gl 6 cannot build a `Request` from
  a relative URL.
- Tiles are served **without** caching headers by the BFF proxy; there is no
  tile archive cache either.
