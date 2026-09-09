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

**Caching.** The archive changes only when the cron `tiles_update` job rebuilds
it (every ~2 months, atomically), so nginx serves `/tiles/map.pmtiles` with a
long `Cache-Control: public, max-age=604800, must-revalidate` (7 days) and its
default `ETag` + `Last-Modified`. A browser serves the archive from cache for a
week and then revalidates cheaply with `If-None-Match` / `If-Modified-Since`
(nginx answers `304 Not Modified` without re-streaming the multi-GB file), and
reads individual tiles via range requests as usual.

## What's in `map.pmtiles`

A single archive combining three *extracts* (not full downloads) of the public
[Protomaps basemap](https://docs.protomaps.com/basemaps/downloads)
(OpenStreetMap-derived, continuously updated, free daily builds at
`build.protomaps.com`):

- A **worldwide** low-zoom sub-pyramid (z0-5) — real coastlines everywhere on
  Earth, not just Germany, so the map is never blank when zoomed out or when
  panning outside Germany.
- A **surroundings** detail extract around Germany (z6-7, bbox
  `-11.112889,43.555498,27.187828,57.470545` — a wide Western/Central Europe
  box) — real z6-7 detail for the area around Germany (Benelux, France,
  Denmark, Poland, Czechia, Austria, ...) so the Germany bbox edge is not
  visible as a seam at mid zoom. Two zoom layers beyond the world backdrop
  (z0-5). The box fully contains Germany, so it also covers Germany at z6-7.
- A **Germany-only** detail extract (z8-15, bbox `5.8,47.2,15.1,55.1`) — full
  street-level OSM detail (roads, buildings, boundaries) for the integrated
  cities (Münster, Bonn, Hamburg). It starts at z8 so the three extracts form
  **disjoint zoom bands** (z0-5 / z6-7 / z8-15), which `pmtiles merge` requires
  — it refuses overlapping inputs. Germany at z6-7 comes from the surroundings
  extract (identical source tiles).

The style
([`frontend/public/styles/basemap.json`](../frontend/public/styles/basemap.json))
uses the real [Protomaps basemap layer schema](https://docs.protomaps.com/basemaps/layers)
(`earth`, `water`, `landcover`, `landuse`, `roads`, `boundaries`, `buildings`)
— fills and lines only, no labels/POIs (which would need the `glyphs`/`sprite`
assets Protomaps hosts externally; omitted to keep the map fully self-hosted at
runtime).

## Building `tiles/map.pmtiles`

The basemap is **mandatory** and built by the backend itself (a Rust driven
adapter, [`backend/src/adapter/driven/tiles_init/`](../backend/src/adapter/driven/tiles_init/)):
it downloads the pinned
[`pmtiles` CLI](https://github.com/protomaps/go-pmtiles) (version from the
`[maps]` TOML section) and runs three `pmtiles extract` calls against the
pinned Protomaps build plus a `pmtiles merge`
(`world.pmtiles` → `surroundings.pmtiles` → `germany.pmtiles` → `map.pmtiles`).
The extracts are three disjoint zoom bands (z0-5 / z6-7 / z8-15), which the
merge requires.

The build no longer blocks startup: the backend binds its HTTP server and
reports healthy immediately (database migrations + startup sync are the only
prerequisites), and the cron-scheduled `tiles_update` job runs the build in the
background on first boot or whenever `tiles/map.pmtiles` is missing (e.g. a
wiped tiles directory). Until it finishes, the frontend shows a "Downloading
map…" loading state over the map; the basemap appears once the job has swapped
the archive into place.

To build it out-of-band (e.g. before the first `docker compose up`):

```bash
make tiles   # docker compose run --rm --no-deps backend tiles -> tiles/map.pmtiles
```

Once `tiles/map.pmtiles` exists it is reused (cached across runs). Refreshing
it is the job of the cron-scheduled `tiles_update` job (see
[below](#updating-the-pinned-protomaps-build)), which rebuilds into a temporary
file and swaps it in atomically so the running app stays online.

### Updating the pinned Protomaps build

The backend downloads a **dated** Protomaps daily build
(`protomaps_build_url` in the `[maps]` section of
[`config.toml`](../config.toml.example)), not a "latest" alias — Protomaps
publishes dated snapshots and explicitly discourages hotlinking them in
production. This is a **provisioning-time only** input (the running app never
talks to Protomaps — see below), but the pinned date should still be refreshed
periodically:

1. Check the current builds at <https://maps.protomaps.com/builds/> for a
   recent `YYYYMMDD.pmtiles` filename.
2. Update `protomaps_build_url` in the `[maps]` section of `config.toml`.
3. `make tiles-update` to rebuild from the new pin, or wait for the next
   `tiles_update` cron run, which applies it atomically.

### Rebuilding from a fresh extract

```bash
make tiles-update   # drops the cached tiles/map.pmtiles, re-runs the tiles build
```

## Resilience

Extraction only happens when provisioning (the initial background build or the
scheduled `tiles_update` run), exactly like the previous
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
