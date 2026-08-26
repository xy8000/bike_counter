# 38 - Bike icon branding + builtin asset folder sync

Status: implemented

## Problem

The header brand uses an emoji (🚴) and the app has no favicon. Stations without a
provider image fall back to a JPEG placeholder (`station-placeholder.jpg`), and the
built-in asset sync only ever **adds** assets — it never removes ones that were
deleted from the assets folder. The Leaflet zoom control renders on the left,
where it is hidden behind the overlay sidebar.

## Goal

1. Use the new white-circle bike icon as the header brand icon and browser
   favicon, and the plain (transparent) bike icon as the default image for
   stations without a provider image.
2. Recolor both bike SVGs from black to the header brand colour (emerald-600
   `#059669`, matching `--primary` in
   [`frontend/src/index.css`](../frontend/src/index.css:14)).
3. Remove `station-placeholder.jpg` from the repo **and** from object storage
   (MinIO) + the `assets` table by making the builtin sync reconcile the folder
   contents (add + remove).
4. Move the map zoom control to the right so it is no longer hidden behind the
   sidebar.

## Scope decision (confirmed with user)

- Recolor **both** bike SVGs (white-circle icon and plain transparent icon) to
  emerald-600 `#059669`.

## Approach

### Backend — builtin asset registry

[`builtin_images()`](../backend/src/main.rs:46) is the compiled-in representation
of the tracked contents of [`backend/assets/`](../backend/assets). Each entry maps
a file in that folder to an `object_key` (prefix `builtin/`) and embeds the bytes
via `include_bytes!`.

- Add the white-circle icon:
  - `object_key`: `builtin/bike-icon-white-circle.svg`
  - `content_type`: `image/svg+xml`
- Add the plain bike icon and make it the new default:
  - `object_key`: `builtin/bike-icon-black-transparent.svg`
  - `content_type`: `image/svg+xml`
- Repoint
  [`DEFAULT_IMAGE_OBJECT_KEY`](../backend/src/core/application/asset_service.rs:25)
  from `builtin/station-placeholder.jpg` to
  `builtin/bike-icon-black-transparent.svg`.
- Delete [`backend/assets/station-placeholder.jpg`](../backend/assets/station-placeholder.jpg)
  and remove its entry from `builtin_images()`.

Note: the filename `bike-icon-black-transparent.svg` becomes slightly misleading
once recolored emerald, but the file name is kept as the user provided it (the
object key mirrors the file name so the folder↔bucket mapping stays obvious).

### Backend — sync add + remove

[`AssetService::sync_builtin_images()`](../backend/src/core/application/asset_service.rs:87)
currently only uploads/updates. Extend it to reconcile both directions:

1. Keep the existing add/update loop (idempotent on object key + sha256).
2. Build the set of desired builtin object keys from the `builtin` slice.
3. Read all assets via `repository.list()`, filter to
   `AssetOrigin::Builtin`, and for every builtin asset whose object key is **not**
   in the desired set:
   - `repository.delete(object_key)` (the `assets` row), then
   - `storage.delete(object_key)` (the S3 object).

   Ordering: delete the DB row first so a crash leaves only an orphan object,
   which the existing
   [`AssetCleanupService`](../backend/src/core/application/asset_cleanup_service.rs:36)
   already removes on its cron schedule (the reverse order would leave a dangling
   DB row pointing at missing content).

To support this, add a `delete` method to the
[`AssetRepository`](../backend/src/core/domain/assets/repository_port.rs:10) driven
port and implement it in
[`PostgresAssetRepository`](../backend/src/adapter/driven/postgres/asset_repository.rs:53)
as `DELETE FROM assets WHERE object_key = $1`. The existing FK on
`counting_stations.image_asset_id` (`ON DELETE SET NULL`, see
[`V12__add_assets.sql`](../backend/migrations/V12__add_assets.sql:18)) unlinks
stations automatically; the BFF already falls back to
[`default_asset()`](../backend/src/adapter/driving/bff/handlers.rs:246) when a
station has no linked asset, and the import job re-links stations to the new
default on its next run.

### Backend — tests

- Update the memory `AssetRepository` mocks and the asset-cleanup test fixtures
  that hard-code `builtin/station-placeholder.jpg`.
- Add unit tests for the removal path in `sync_builtin_images` (a builtin asset in
  the repo but not in the passed slice gets deleted from both storage and repo;
  provider assets are never removed).
- Add a repository-level test for `delete` (row removed; `ON DELETE SET NULL`
  behaviour on `counting_stations` is covered by the existing migration test if
  present, otherwise a targeted assertion).
- Keep `make coverage` green (core at/above 95%, overall at/above 80%).

### Frontend — favicon + header icon

- Create [`frontend/public/`](../frontend/public) with a copy of the recolored
  white-circle bike SVG (e.g. `bike-icon.svg`).
- Add `<link rel="icon" type="image/svg+xml" href="/bike-icon.svg" />` to
  [`frontend/index.html`](../frontend/index.html:1).
- In [`TopBar.tsx`](../frontend/src/features/header/TopBar.tsx:12), replace the
  `🚴` emoji span with an `<img src="/bike-icon.svg" alt="" />` sized to the header
  height (the SVG is a 76x76 white circle, so constrain it with a Tailwind size
  class, e.g. `h-8 w-8`).

### Frontend — map controls

In [`MapView.tsx`](../frontend/src/features/map/MapView.tsx:45), move the Leaflet
zoom control from the default top-left to the top-right so it is not covered by
the overlay sidebar:

- Disable the default control: `zoomControl={false}` on `<MapContainer>`.
- Render `<ZoomControl position="topright" />` as a child (import from
  `react-leaflet`).

The detail-page preview
([`DetailMap.tsx`](../frontend/src/features/stationDetail/DetailMap.tsx:48)) already
sets `zoomControl={false}`, so it is unaffected.

## Out of scope

- [`map-flag-counting-station.svg`](../backend/assets/map-flag-counting-station.svg)
  is present in the assets folder but not registered in `builtin_images()` and is
  not referenced anywhere; it is left as-is unless the user wants it wired in.
- No auto-scan of the assets directory at build time — the `builtin_images()` list
  remains the explicit source of truth for which files are tracked/synced.

## Files to change

Backend:

- [`backend/assets/bike-icon-white-circle.svg`](../backend/assets/bike-icon-white-circle.svg)
  (recolor bike to `#059669`)
- [`backend/assets/bike-icon-black-transparent.svg`](../backend/assets/bike-icon-black-transparent.svg)
  (recolor bike to `#059669`)
- [`backend/src/main.rs`](../backend/src/main.rs:46) (builtin list)
- [`backend/src/core/application/asset_service.rs`](../backend/src/core/application/asset_service.rs:25)
  (default key + sync removal)
- [`backend/src/core/domain/assets/repository_port.rs`](../backend/src/core/domain/assets/repository_port.rs:10)
  (add `delete`)
- [`backend/src/adapter/driven/postgres/asset_repository.rs`](../backend/src/adapter/driven/postgres/asset_repository.rs:53)
  (implement `delete` + tests)
- Delete [`backend/assets/station-placeholder.jpg`](../backend/assets/station-placeholder.jpg)

Frontend:

- [`frontend/public/bike-icon.svg`](../frontend/public/bike-icon.svg) (new)
- [`frontend/index.html`](../frontend/index.html:1) (favicon link)
- [`frontend/src/features/header/TopBar.tsx`](../frontend/src/features/header/TopBar.tsx:12)
  (brand icon)
- [`frontend/src/features/map/MapView.tsx`](../frontend/src/features/map/MapView.tsx:45)
  (zoom control position)

Docs:

- [`plans/README.md`](../plans/README.md:1) (register this plan)
- [`README.md`](../README.md:177) (builtin asset description)
- [`ToDo.md`](../ToDo.md) (task notes)

## Definition of done

- Both bike SVGs recolored to `#059669`.
- Header shows the bike icon, browser tab shows the favicon, stations without a
  provider image show the plain bike icon.
- `station-placeholder.jpg` gone from repo and from MinIO + `assets` after startup.
- Builtin sync adds **and** removes assets to mirror the folder contents.
- Map zoom control sits on the right, clear of the sidebar.
- `make check`, `make test`, `make coverage` green; `make test-playwright` green
  (frontend UI touched).

## Implementation notes

- **Empty-radar crash fixed (found by the e2e gate).** The detail page crashed
  (`Cannot read properties of null (reading 'map')` inside recharts'
  `RadarChart`) when the selected window had no data (e.g. today is empty for a
  still-importing dataset). [`WeekdayRadar`](../frontend/src/features/stationDetail/WeekdayRadar.tsx:32)
  now renders the same "No traffic for this period." empty state as the other
  charts when no series has a non-zero weekday total, instead of feeding recharts
  empty rows.
- **e2e compare-previous assertion made robust.** [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:148)
  asserted a "Day before" legend entry appears after enabling "Compare previous
  period", which only holds when the current day has data. When today is empty
  the previous period draws as the single series and the legend is intentionally
  hidden (plan 36), so the test now accepts either the legend entry or a non-empty
  chart.
- **Gate results:** `make check` ✓, `make test` (334) ✓, `make test-rest` (82) ✓,
  `make coverage` ✓ (overall 83.78%, core 95.38%), `make frontend-build` ✓,
  `make test-playwright` (14) ✓.
