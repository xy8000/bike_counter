# Self-hosted PMTiles basemap

This directory holds the **data** for the self-hosted vector basemap: a single
`tiles/map.pmtiles` archive, git-ignored because it's built once and can be
large. The `frontend` docker-compose service mounts this folder read-only into
nginx (`/usr/share/nginx/html/tiles`), which serves it as a plain static file
at `/tiles/map.pmtiles`.

There is **no tile-server process**. MapLibre reads individual tiles directly
out of the static archive via HTTP range requests, using the `pmtiles` npm
package's protocol handler (registered in
[`frontend/src/lib/map.tsx`](../frontend/src/lib/map.tsx)) — this is the
"serverless PMTiles" pattern used by Protomaps/OpenFreeMap-based deployments.
nginx supports byte-range requests for static files by default, so no special
config is needed beyond the `/tiles/` location in
[`nginx.conf.template`](../frontend/nginx.conf.template).

## What's in `map.pmtiles`

A single archive combining two *extracts* (not full downloads) of the public
[Protomaps basemap](https://docs.protomaps.com/basemaps/downloads)
(OpenStreetMap-derived, continuously updated, free daily builds at
`build.protomaps.com`):

- A **worldwide** low-zoom sub-pyramid (z0-5) — real coastlines everywhere on
  Earth, not just Germany, so the map is never blank when zoomed out or when
  panning outside Germany.
- A **Germany-only** detail extract (z6-15, bbox `5.8,47.2,15.1,55.1`) — full
  street-level OSM detail (roads, buildings, boundaries) for the integrated
  cities (Münster, Bonn, Hamburg).

These two zoom ranges don't overlap, so they can be combined into one archive
with `pmtiles merge`. The style
([`frontend/public/styles/basemap.json`](../frontend/public/styles/basemap.json))
uses the real [Protomaps basemap layer schema](https://docs.protomaps.com/basemaps/layers)
(`earth`, `water`, `landcover`, `landuse`, `roads`, `boundaries`, `buildings`)
— fills and lines only, no labels/POIs (which would need the `glyphs`/`sprite`
assets Protomaps hosts externally; omitted to keep the map fully self-hosted at
runtime).

## Building `tiles/map.pmtiles`

```bash
make tiles   # docker compose up tiles -> tiles/map.pmtiles
```

`docker compose up`/`make run` build it automatically via the one-shot `tiles`
init container (same shape as the old `minio-init`): it downloads the pinned
[`pmtiles` CLI](https://github.com/protomaps/go-pmtiles) release binary, runs
two `pmtiles extract` calls against the pinned Protomaps build plus a
`pmtiles merge`, and is skipped entirely once `tiles/map.pmtiles` exists
(cached across runs), or via `SKIP_TILES=1` (used by the e2e when the basemap
isn't needed).

### Updating the pinned Protomaps build

The `tiles` init container downloads a **dated** Protomaps daily build
(`PROTOMAPS_BUILD_URL` in [`docker-compose.yml`](../docker-compose.yml)), not
a "latest" alias — Protomaps publishes dated snapshots and explicitly
discourages hotlinking them in production. This is a **build-time only**
input (the running app never talks to Protomaps — see below), but the pinned
date should still be refreshed periodically:

1. Check the current builds at <https://maps.protomaps.com/builds/> for a
   recent `YYYYMMDD.pmtiles` filename.
2. Update `PROTOMAPS_BUILD_URL` in `docker-compose.yml` (or set the
   environment variable to override it without editing the file).
3. `make tiles-update` to rebuild from the new pin.

### Rebuilding from a fresh extract

```bash
make tiles-update   # drops the cached tiles/map.pmtiles, re-runs the tiles init container
```

## Resilience

Extraction only happens at build time, exactly like the previous
Geofabrik/Planetiler download this replaced: once `tiles/map.pmtiles` exists,
the running app never makes another request to Protomaps. If
`build.protomaps.com` becomes unreachable, only building/rebuilding the
archive fails — an already-built `tiles/map.pmtiles` keeps working
indefinitely.

## After provisioning

```bash
docker compose up -d --build frontend
```

Then verify the archive is reachable through nginx:

```bash
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' \
  http://localhost:8081/tiles/map.pmtiles
```

Should print `200 application/octet-stream` (or `206` if requested with a
`Range` header, which is how MapLibre actually reads it).

## Notes

- The frontend uses `maplibre-gl@^6`. maplibre-gl 6 parses tiles in a separate
  Web Worker (`maplibre-gl-worker.mjs`, which imports
  `./maplibre-gl-shared.mjs`); the bundler does not emit it on its own, so
  [`lib/map.tsx`](../frontend/src/lib/map.tsx) bundles it with Vite
  (`?worker&url`) and registers it via `setWorkerUrl()` before any map is
  created — without this, no tiles load.
- Previous architectures (git history): plan 59/60/61 ran a `martin` tile
  server behind a BFF proxy (`/api/map/*`) serving a hand-rolled Natural Earth
  MVT generator plus a Planetiler/Geofabrik Germany build; plan 64 replaced
  all of that with the static-file approach documented here after the custom
  MVT generator produced several rendering bugs (wrong ring winding,
  antimeridian-wrapped polygons, overzoom artifacts) — see
  [`plans/64_serverless_pmtiles_basemap_plan.md`](../plans/64_serverless_pmtiles_basemap_plan.md).
