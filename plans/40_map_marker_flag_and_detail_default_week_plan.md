# 40 - Map-marker flag relocation + detail-page default week

Status: implemented

## Problem

The map and detail-preview markers are inline SVG data-URLs baked into
[`frontend/src/lib/leaflet.ts`](../frontend/src/lib/leaflet.ts:20), so changing
the marker artwork requires a code edit. The user-added
[`backend/assets/map-flag-counting-station.svg`](../backend/assets/map-flag-counting-station.svg:1)
(a teardrop pin with a bike inside) is still black, is only synced to object
storage by the compile-time folder scan, and is not referenced anywhere. The
detail page defaults to the 24-hour timeframe, which can be empty for a
still-importing dataset.

## Goal

1. Use the map-flag (pin + bike) as the marker for **both** the main map and the
   detail-page map preview, recolored to the brand emerald (matching the bike
   icon and the current markers).
2. Relocate the marker to the frontend as a real asset file so editing the SVG
   automatically updates the markers (no code change).
3. Change the detail-page default timeframe from 24 hours to the week timeframe.

## Scope decision (confirmed with user)

- Default timeframe = the existing `week` option ("Current + last week"), because
  the 24-hour window is not always populated.
- The recolored flag replaces **both** [`stationIcon`](../frontend/src/lib/leaflet.ts:26)
  and [`detailStationIcon`](../frontend/src/lib/leaflet.ts:45).

## Approach

### Frontend — relocate + recolor the marker

- Create `frontend/src/features/map/map-flag-counting-station.svg` as a copy of
  the backend asset with every `#000000` fill replaced by the brand emerald
  `#059669` (the white circle and the layout stay). The bike glyph inside the pin
  also becomes `#059669`.
- In [`leaflet.ts`](../frontend/src/lib/leaflet.ts:20), replace the two inline
  `markerSvg`/`detailMarkerSvg` strings with a Vite asset import:
  `import markerUrl from '../features/map/map-flag-counting-station.svg'`.
  - `stationIcon` uses `iconUrl: markerUrl` with `iconSize: [32, 40]` and the
    anchor on the pin's bottom tip (the SVG is 32x40 with the tip at y≈37).
  - `detailStationIcon` reuses the same URL, rendered larger (e.g. `iconSize:
    [44, 55]`) so the single station still reads as "highlighted"; the previous
    halo circle is dropped in favor of the shared flag design.
  - Keep the Leaflet `shadowUrl` and the existing `popupAnchor`/`alt`/`title`
    wiring so popup positioning and the Playwright locators keep working.

Because the SVG is imported through Vite, editing the file in the feature is
enough to update the marker on rebuild/HMR — the "automatically updated" request.

### Frontend — detail default timeframe

In [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:320),
change the initial state from `useState<Timeframe>('day')` to
`useState<Timeframe>('week')`.

### Frontend — e2e updates

- [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:134): the default-bucket
  assertion changes from "5-minute buckets" to the week subtitle
  "1-hour buckets — the weeks are overlapped".
- [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:148): the compare-previous
  test now expects the week default — no "Last week" legend entry before the
  checkbox, and either a "Last week" legend entry or a non-empty chart after
  checking it (same single-series robustness as today).

### Backend — stop syncing the flag

Delete [`backend/assets/map-flag-counting-station.svg`](../backend/assets/map-flag-counting-station.svg:1).
The compile-time folder scan in [`backend/src/main.rs`](../backend/src/main.rs:53)
then stops bundling it, and the existing add/remove reconciliation in
[`sync_builtin_images`](../backend/src/core/application/asset_service.rs:88)
removes the stale `builtin/map-flag-counting-station.svg` object from MinIO and
the `assets` table on the next startup. No backend code change is required.

## Out of scope

- No new timeframe option: "last week" is the existing "Current + last week"
  week timeframe.
- No marker-upload API; the marker is a static frontend asset served by nginx.

## Files to change

Frontend:

- [`frontend/src/features/map/map-flag-counting-station.svg`](../frontend/src/features/map/map-flag-counting-station.svg)
  (new, recolored `#059669`)
- [`frontend/src/lib/leaflet.ts`](../frontend/src/lib/leaflet.ts:20)
  (asset import + both icons)
- [`frontend/src/features/stationDetail/StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:320)
  (default `'week'`)
- [`frontend/e2e/detail.spec.ts`](../frontend/e2e/detail.spec.ts:134)
  (default-bucket + compare assertions)
- [`frontend/e2e/sidebar.spec.ts`](../frontend/e2e/sidebar.spec.ts:30)
  (focus-click moved to map void — see implementation notes)

Backend:

- Delete [`backend/assets/map-flag-counting-station.svg`](../backend/assets/map-flag-counting-station.svg:1)

Docs:

- [`plans/README.md`](../plans/README.md:1) (register this plan)
- [`README.md`](../README.md:162), [`ToDo.md`](../ToDo.md) (asset-convention notes)

## Definition of done

- The map and detail-preview markers render the recolored flag (pin + bike) in
  emerald `#059669`.
- Editing the marker SVG in `frontend/src/features/map/` updates the markers
  without touching `leaflet.ts`.
- The detail page opens on the week timeframe by default.
- The backend no longer syncs `map-flag-counting-station.svg` to object storage.
- `make check`, `make test`, `make coverage`, `make frontend-build` and
  `make test-playwright` green.

## Implementation notes

- **Wider marker hit area broke the sidebar e2e (found by the e2e gate).** The
  map marker is now 32 px wide (was 25 px), so its hit area covers the map
  focus-click at `(700, 300)` used by
  [`sidebar.spec.ts`](../frontend/e2e/sidebar.spec.ts:30) — the click opened the
  station overview, which replaces the sidebar, and the zoom poll timed out.
  The focus-click now lands on map "void" (a far corner, clear of the central
  Münster cluster) with a guard that closes the overview if one still opens.
- **Malformed SVG comment (found by the user, not the e2e gate).** The marker SVG
  initially carried a comment mentioning `--primary` — a double hyphen is illegal
  inside an XML/SVG comment, so the file failed to parse and the marker image
  rendered broken. The comment was reworded to avoid `--` and the file is now
  validated as well-formed XML. The e2e gate does not catch this because it only
  asserts the marker `<img>` element exists and is clickable, not that the image
  decodes.
- **Gate results:** `make check` ✓, `make test` (337) ✓, `make test-rest` (82) ✓,
  `make coverage` ✓ (overall 84.08%, core 95.38%), `make frontend-build` ✓,
  `make test-playwright` (14) ✓.
