# 39 - Frontend resilience + builtin asset folder sync

Status: implemented

## Problem

Plan 38 was a small feature but took a long time because two latent frontend
defects only surfaced under edge-case data (an empty "current day" window), and
both produced symptoms far from the real cause:

- A single recharts crash blanked the **entire** detail page (header, search and
  all) because there is no React error boundary. The symptom was "blank page"
  instead of "one broken chart".
- [`WeekdayRadar`](../frontend/src/features/stationDetail/WeekdayRadar.tsx:39) was
  the only chart without an empty-state guard, while
  [`TimeSeriesLineChart`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:69)
  and [`ChannelPie`](../frontend/src/features/stationDetail/ChannelPie.tsx:36)
  already had one.
- The built-in assets are a hand-maintained list in
  [`builtin_images()`](../backend/src/main.rs:46), not a scan of
  [`backend/assets/`](../backend/assets), so files can drift out of sync with the
  bucket (as `map-flag-counting-station.svg` already has).
- The brand bike icon is duplicated between
  [`backend/assets/bike-icon-white-circle.svg`](../backend/assets/bike-icon-white-circle.svg:1)
  and [`frontend/public/bike-icon.svg`](../frontend/public/bike-icon.svg:1).

The e2e stack is intentionally kept on **persistent volumes** (no `docker compose
down -v`) so the real Münster import is not re-run on every gate — that behaviour
stays unchanged; its only cost was the now-fixed data-dependent assertions from
plan 38.

## Goal

1. Contain chart/render crashes with a React error boundary so the rest of the
   page (header/search) keeps working and the failure is self-describing.
2. Give every chart a single, consistent empty-state guard.
3. Derive the built-in asset list by scanning `backend/assets/` at compile time,
   so adding/removing a file in the folder is the only step needed to change what
   is synced to S3.
4. Keep exactly one copy of the brand bike icon.

## Approach

### 1 - React error boundary (frontend)

- Add a small class component [`frontend/src/lib/ErrorBoundary.tsx`](../frontend/src/lib/ErrorBoundary.tsx)
  implementing `getDerivedStateFromError` + `componentDidCatch`, with an optional
  `fallback` prop and a muted default fallback card
  ("Something went wrong rendering this section.").
- Wrap the detail-page content (the `<DetailContent>` subtree in
  [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:313))
  so the shared header/search render above it is unaffected by a chart crash. The
  boundary lives inside the route, below [`SearchableHeader`](../frontend/src/features/header/SearchableHeader.tsx:11).
- Optionally also wrap each `ChartCard` child so a single broken chart degrades
  while sibling charts stay visible; the primary requirement is that a crash never
  blanks the whole page.

### 2 - Unified chart empty state (frontend)

- Add a shared [`frontend/src/features/stationDetail/ChartEmptyState.tsx`](../frontend/src/features/stationDetail/ChartEmptyState.tsx)
  component: a standard aspect-ratio container + muted message (reuse the existing
  "No data for this period." / "No traffic for this period." wording), so all chart
  wrappers look identical when empty.
- Refactor [`TimeSeriesLineChart`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:69),
  [`ChannelPie`](../frontend/src/features/stationDetail/ChannelPie.tsx:36) and
  [`WeekdayRadar`](../frontend/src/features/stationDetail/WeekdayRadar.tsx:39) to
  use it, and ensure each guards both "no data" and "all-zero data".

### 4 - Builtin asset folder scan (backend)

- Add a crate that embeds a directory at compile time (e.g. `include_dir`) and
  point it at [`backend/assets/`](../backend/assets).
- Replace [`builtin_images()`](../backend/src/main.rs:46) with iteration over the
  embedded directory: for each file build a `BuiltinImage` with
  `object_key = "builtin/{relative path}"`, a content type derived from the file
  extension (new forward helper, e.g. `.svg` → `image/svg+xml`, `.jpg` →
  `image/jpeg`, `.png` → `image/png`, `.webp` → `image/webp`) and the file bytes.
- Keep the default fallback key as a constant built from a well-known filename
  (the plain bike icon), e.g. `builtin/{DEFAULT_IMAGE_FILENAME}`; the existing
  [`DEFAULT_IMAGE_OBJECT_KEY`](../backend/src/core/application/asset_service.rs:26)
  logic in [`default_asset()`](../backend/src/core/application/asset_service.rs:108)
  is unchanged.
- The existing add/remove reconciliation in
  [`sync_builtin_images()`](../backend/src/core/application/asset_service.rs:88)
  then automatically uploads new files and deletes removed ones — the folder
  becomes the single source of truth. `map-flag-counting-station.svg` will start
  being synced (harmless, matches "sync the folder").
- Unit-test the content-type-from-extension mapping and the object-key derivation.

### 5 - Single brand icon

- The white-circle bike icon is only consumed by the frontend (favicon +
  [`TopBar`](../frontend/src/features/header/TopBar.tsx:13)); nothing streams it
  through the BFF. Consolidate on the single frontend copy
  [`frontend/public/bike-icon.svg`](../frontend/public/bike-icon.svg:1) and remove
  [`backend/assets/bike-icon-white-circle.svg`](../backend/assets/bike-icon-white-circle.svg:1)
  (the folder scan then stops syncing it automatically).
- Keep the plain bike icon as the backend station-default image in
  [`backend/assets/bike-icon-black-transparent.svg`](../backend/assets/bike-icon-black-transparent.svg:1).
  Optionally rename it to drop the now-misleading "black" (e.g. `bike-icon.svg`),
  updating the default-filename constant; the folder scan + add/remove sync
  reconcile the object key automatically.
- Document the convention in [`README.md`](../README.md:162): frontend brand assets
  live in [`frontend/public/`](../frontend/public), backend builtin assets (served
  via the BFF) live in [`backend/assets/`](../backend/assets).

## Out of scope

- No change to the e2e volume handling: `scripts/e2e-playwright.sh` keeps the
  persistent Postgres/MinIO volumes so the real Münster import is not re-run each
  gate. Determinism against a persisted dataset is handled by robust assertions
  (plan 38).
- No new upload/delete API; the built-in sync remains the only mechanism.

## Files to change

- [`frontend/src/lib/ErrorBoundary.tsx`](../frontend/src/lib/ErrorBoundary.tsx) (new)
- [`frontend/src/features/stationDetail/StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:313) (wrap content)
- [`frontend/src/features/stationDetail/ChartEmptyState.tsx`](../frontend/src/features/stationDetail/ChartEmptyState.tsx) (new)
- [`frontend/src/features/stationDetail/TimeSeriesLineChart.tsx`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:69)
- [`frontend/src/features/stationDetail/ChannelPie.tsx`](../frontend/src/features/stationDetail/ChannelPie.tsx:36)
- [`frontend/src/features/stationDetail/WeekdayRadar.tsx`](../frontend/src/features/stationDetail/WeekdayRadar.tsx:39)
- [`backend/Cargo.toml`](../backend/Cargo.toml) (`include_dir`)
- [`backend/src/main.rs`](../backend/src/main.rs:46) (folder scan replaces the list)
- [`backend/src/core/application/asset_service.rs`](../backend/src/core/application/asset_service.rs:26) (default-key constant + content-type helper/tests)
- Delete [`backend/assets/bike-icon-white-circle.svg`](../backend/assets/bike-icon-white-circle.svg)
- [`README.md`](../README.md:162), [`ToDo.md`](../ToDo.md), [`plans/README.md`](../plans/README.md:15)

## Definition of done

- A chart crash on the detail page shows an inline error while the header/search
  keep working (no full-page blank).
- All chart wrappers share one empty-state component and guard empty/all-zero data.
- `backend/assets/` is scanned at compile time; adding/removing a file there is
  reflected in the builtin sync without editing `builtin_images()`.
- Exactly one copy of the brand white-circle icon remains (frontend).
- `make check`, `make test`, `make coverage`, `make frontend-build` and
  `make test-playwright` green (e2e stays fast on persistent volumes).
