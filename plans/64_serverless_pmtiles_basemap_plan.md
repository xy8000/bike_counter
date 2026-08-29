# 64 - Serverless PMTiles basemap (drop Martin + the custom tile generator)

Status: implemented

## Problem

Plans 59/61/63 built a self-hosted basemap around a **Martin** tile-server
container plus a hand-rolled Node script
(`frontend/scripts/build-world-tiles.mjs`) that re-implemented Web Mercator
projection, Sutherland-Hodgman polygon clipping and MVT encoding from scratch
to produce the coarse world backdrop. That custom code produced three separate
rendering bugs in one session (wrong ring winding, antimeridian-wrapped
polygons leaking into unrelated tiles, and a hard-coded `maxzoom` removal that
overzoomed the crude z0-5 data far past its native resolution). Running a
dedicated tile-server process is also heavier than this app needs.

## Approach

Replace the whole pipeline with the **serverless PMTiles** pattern (the same
one used by Protomaps/OpenFreeMap-based OSS deployments): a single static
`.pmtiles` file, read directly by the browser via HTTP range requests, no
tile-server process and no hand-written tile-generation code.

- **Data source:** [Protomaps](https://protomaps.com)' publicly hosted,
  continuously-updated planet-wide vector basemap (OpenStreetMap-derived,
  `docs.protomaps.com/basemaps/layers` schema: `earth`, `water`, `landcover`,
  `landuse`, `roads`, `boundaries`, `buildings`, ...). Extraction (not a full
  download) is done once with the official `pmtiles` CLI
  (`github.com/protomaps/go-pmtiles`), which fetches only the requested tiles
  over HTTP range requests:
  - `pmtiles extract $SOURCE world.pmtiles --maxzoom=5` — a full low-zoom
    sub-pyramid, i.e. a coarse backdrop for the **entire planet** (real
    coastlines everywhere, not just Germany).
  - `pmtiles extract $SOURCE germany.pmtiles --bbox=<germany> --minzoom=6 --maxzoom=15`
    (extended from z14 to z15 — the source build's max zoom — so the high-zoom
    water/canal geometry is the accurate z15 version instead of overzoomed z14
    generalization blobs; this is what fixes the visible water artifacts)
    — Germany-only detail from z6 up (disjoint zoom range from the world
    extract, so the two archives don't overlap).
  - `pmtiles merge world.pmtiles germany.pmtiles map.pmtiles` — combines them
    into the single archive the frontend serves.
  - This is a one-shot `tiles` init container (same shape as the old
    `basemap`/`minio-init` init containers): it downloads the pinned
    `go-pmtiles` release binary, runs the three commands above, and is skipped
    entirely once `tiles/map.pmtiles` exists (or via `SKIP_TILES=1`).
- **Serving:** nginx serves `tiles/map.pmtiles` as a plain static file at
  `/tiles/map.pmtiles` (byte-range support is nginx's default static-file
  behavior — no config needed). This removes the `martin` docker service, the
  `tile_network`, and the BFF's `/api/map/*` proxy
  (`backend/src/adapter/driving/rest/tiles.rs`) entirely.
- **Reading:** the frontend registers the `pmtiles://` protocol via the
  official `pmtiles` npm package (`Protocol` + `maplibregl.addProtocol`), so
  MapLibre reads tiles directly out of the static file with range requests —
  no server-side tiling logic anywhere in this app.
- **Style:** `frontend/public/styles/basemap.json` is rewritten to use the
  real Protomaps schema layer names (was previously hand-guessing OpenMapTiles
  names for a source Planetiler never actually produced in that shape for the
  world layer). No labels/glyphs/sprites are added — this keeps the same
  visual scope as before (fills + lines only), so no new external runtime
  asset dependency (glyphs/sprites) is introduced.

### Resilience note

Extraction only happens at build time (like the previous Geofabrik download);
the running app never talks to Protomaps. If `build.protomaps.com` is
unreachable when `tiles/map.pmtiles` doesn't exist yet, only the *build* step
fails (same failure mode the old Planetiler/Geofabrik step had) — already
extracted files keep working indefinitely. Protomaps' docs explicitly
recommend not hotlinking their daily build in production; the pinned date in
the `tiles` init container is a build-time input only (`PROTOMAPS_BUILD_URL`
env var), documented in `tiles/README.md` with instructions to bump it
periodically from `maps.protomaps.com/builds`.

### Removed

- The `martin` docker-compose service and the `tile_network`.
- `backend/src/adapter/driving/rest/tiles.rs` (the BFF tile proxy) and its
  route/tests.
- `frontend/scripts/build-world-tiles.mjs` (the buggy custom MVT generator)
  and its now-unused `topojson-client`/`vt-pbf`/`world-atlas` dependencies.
- `frontend/scripts/probe-tiles.mjs`, `render-standalone.mjs`,
  `tile-diagnostics.mjs` — debug scripts written against the old `/api/map/*`
  proxy.
- `scripts/build_tiles.sh` and the `basemap`/Planetiler/Geofabrik init
  container — superseded by the Protomaps extract, which needs no Java, no
  3.5 GB extract download, and no per-repo Planetiler profile. The previous
  architecture remains available in git history if Protomaps' free extract
  service ever becomes unusable.

## Validation

- Rebuilt the stack (`docker compose up -d --build`), confirmed `tiles/map.pmtiles`
  is produced by the new `tiles` init container and the frontend serves it at
  `/tiles/map.pmtiles`.
- Verified in the browser: Germany still renders full street-level detail;
  non-German areas (previously blank/broken) now show the real global
  coastline backdrop, at both low zoom (whole-Europe view) and overzoomed
  high-zoom views (no more diagonal artifacts).
- Ran the repository gates required by `agents.md`.
