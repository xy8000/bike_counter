# 33 - Overview detail-link polish + asset subsystem findings fixes

Status: implemented

## Problem

The plan 32 review surfaced four actionable implementation findings in the new
asset/overview subsystem, plus a UI polish request for the "open detail page"
link. Two review items are resolved by decision (see Scope decisions): the
Bohlweg coordinate tweak is intentional and stays, and the rare "DB row present
but object missing" 404 case is deferred as a documented limitation.

## Goals

1. Reconcile the provider-image hash so the content-addressed object key always
   matches the persisted `sha256` (removes the trust gap in
   [`AssetService::store_provider_image`](../backend/src/core/application/asset_service.rs:117)).
2. Remove the unused `etag` from [`AssetObjectInfo`](../backend/src/core/domain/assets/asset_storage_port.rs:49)
   (and the duplicate hash computation in [`MinioAssetStorage`](../backend/src/adapter/driven/minio_asset_storage.rs:92));
   the BFF keeps deriving `ETag` from the asset's DB `sha256`.
3. Add backpressure to the image stream (bounded channel) so a slow browser
   never buffers the whole object in memory.
4. Avoid the per-station `default_asset()` lookup during import (resolve once).
5. Turn "Open detail page" into an icon-only button next to the "x", and make the
   station-name heading clickable (without link styling) — in **both** the
   overview panel and the map popup.
6. Unify station selection: map marker click, sidebar item click and the search
   "find on map" must all open the **same** overview panel through a single
   `selectStation` entry point (flying to the station first). The duplicate raw
   `L.popup` in [`App.tsx`](../frontend/src/App.tsx:39) is removed.

## Scope decisions

- **Keep the coordinate change**: [`station_metadata.rs`](../backend/src/adapter/driven/muenster_github/station_metadata.rs:90)
  now asserts `51.9688 / 7.6435` for "Bohlweg". The user confirmed these values
  are correct; the change stays (it also fixes the previously failing test).
- **Defer the missing-object 404**: `GET /api/bff/assets/{id}/content` keeps
  returning 500 when the DB row exists but the object is absent from MinIO. This
  can only happen after manual storage tampering (cleanup never deletes known
  keys), so it is documented as a known limitation rather than adding a new
  `DomainError` variant.

## Changes

### Backend — [`asset_service.rs`](../backend/src/core/application/asset_service.rs:117)

`store_provider_image` verifies the passed `sha256` against the bytes: compute
`sha256_of(bytes)` and return `DomainError::InvalidQuery` when it differs from
the passed digest. The object key keeps using `sha256.0`, which is now guaranteed
to equal the content hash and the persisted `sha256` column.

### Backend — asset storage port + adapter + mocks

- [`asset_storage_port.rs`](../backend/src/core/domain/assets/asset_storage_port.rs:49):
  drop the `etag` field from `AssetObjectInfo` (it is never consumed) and update
  the doc comment: the BFF derives response headers (`Content-Type`, `ETag`,
  `Content-Length`) from the asset's DB metadata, so `put` only needs `byte_size`.
- [`minio_asset_storage.rs`](../backend/src/adapter/driven/minio_asset_storage.rs:81):
  remove the `etag`/`Sha2Digest` computation from `put`.
- Update the `AssetStorage` mock implementations that construct
  `AssetObjectInfo` (in [`asset_service.rs`](../backend/src/core/application/asset_service.rs:240),
  [`asset_cleanup_service.rs`](../backend/src/core/application/asset_cleanup_service.rs:323) and
  [`rest/tests/mocks.rs`](../backend/src/adapter/driving/rest/tests/mocks.rs:1)).

### Backend — streaming backpressure — [`minio_asset_storage.rs`](../backend/src/adapter/driven/minio_asset_storage.rs:129)

Replace the `mpsc::unbounded_channel` in `get_stream` with a bounded channel
(`mpsc::channel(N)`, small N) and implement [`ChunkWriter`](../backend/src/adapter/driven/minio_asset_storage.rs:158)
over the sender's `Sink` API (`poll_ready` / `start_send` / `poll_flush`) so
`poll_write` returns `Poll::Pending` when the buffer is full and is woken when
the consumer drains it.

### Backend — import N+1 — [`data_import_service.rs`](../backend/src/core/application/data_import_service.rs:205)

Resolve `asset_service.default_asset()` once in `sync_counting_stations` (before
the station loop, only when `asset_service` is `Some`) and pass the resolved
`&Asset` into [`sync_station_image`](../backend/src/core/application/data_import_service.rs:208)
instead of looking it up per station.

### Frontend — unified selection — [`App.tsx`](../frontend/src/App.tsx:15)

Replace `focusStation` (which built a raw `L.popup`) with a single
`selectStation({ id, latitude, longitude })` that flies to the station and sets
`selectedStationId`. Map markers, sidebar items and search results all call it,
so selecting a station always opens the same overview panel. `MapView`'s
`onSelectStation` now receives the whole `StationMap` (id + coordinates) instead
of just the id.

### Frontend — overview panel — [`StationOverview.tsx`](../frontend/src/features/stationOverview/StationOverview.tsx:28)

- Top bar becomes `[station name (clickable)] … [ExternalLink icon button] [X close button]`.
- The station name (`<h2>`) becomes an `<a href={overview.detail_url}>` with
  `target="_blank" rel="noreferrer"` and heading styles (no underline, inherit
  color, `cursor-pointer`), keeping `truncate` and the heading font weight.
- Add a ghost icon button (same `size="icon"` as the close button) using the
  existing `ExternalLink` lucide icon with `aria-label="Open detail page"`,
  opening `overview.detail_url` in a new tab.
- Remove the in-body "Open detail page …" text link below the image.

### Frontend — map popup — [`MapView.tsx`](../frontend/src/features/map/MapView.tsx:54)

- Popup content becomes the station name as a clickable link (no link styling,
  `target="_blank" rel="noreferrer"`) plus an `ExternalLink` icon button with
  `aria-label="Open detail page"`, replacing the current
  "Open detail page →" text link.

### Frontend — e2e — [`map.spec.ts`](../frontend/e2e/map.spec.ts:1)

Update the two assertions that target the removed
`getByRole('link', { name: /Open detail page/ })` text links to target the new
icon button (accessible name `Open detail page`) and assert the clickable
heading also links to `/stations/{id}`.

## Task list

See the plan todo list (tracked via `update_todo_list`).

## Verification

- `make check` green (fmt + clippy `-D warnings`).
- `make test` and `make test-rest` green (updated mock constructors + new hash
  verification test in `asset_service`).
- `make coverage` green (core ≥ 95%, overall ≥ 80%).
- `make frontend-build` green.
- `make test-playwright` green (updated overview/popup link assertions).
- Manual: click a marker → overview shows the icon button + clickable heading;
  click the icon or heading → detail link opens; popup shows the same affordance.

## Definition of done

- [x] `store_provider_image` rejects a mismatched hash; object key == persisted sha256.
- [x] `AssetObjectInfo` carries only `byte_size`; adapter/mocks updated.
- [x] Bounded streaming channel with backpressure.
- [x] Import resolves `default_asset()` once per run.
- [x] Icon-only detail link + clickable heading in overview panel and popup.
- [x] Unified map/sidebar/search selection through a single `selectStation`.
- [x] e2e specs updated; `make check`, `make test-rest`, core unit tests and
      `frontend-build` green (Docker gates `make test` / `make coverage` /
      `make test-playwright` still to run).
