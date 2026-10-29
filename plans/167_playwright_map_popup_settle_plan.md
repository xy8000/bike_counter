# 167 - Stabilise the Playwright map-popup detail navigation

Status: implemented

## Problem

The Playwright spec
[`map.spec.ts`](frontend/e2e/map.spec.ts:17) ("clicking a map marker opens a
popup with the station name and detail link") is timing-sensitive. The first
full e2e run of plan 166 failed it: the popup link click left the URL on the map
(`?min_lat=…&station=…`) instead of navigating to `/stations/<id>`. An immediate
rerun was green.

Root cause: clicking a marker calls `selectStation`, which calls
`map.flyTo(...)` and opens a MapLibre popup anchored to that marker. The popup
title/details assertions can resolve while the camera is still flying, so
Playwright may click the detail link while the popup is moving under the
pointer. The click can then land on the map instead of the link (or be consumed
as a map gesture), which is the same class of flake already documented in
[`flags.spec.ts`](frontend/e2e/flags.spec.ts:42) and
[`detail.spec.ts`](frontend/e2e/detail.spec.ts:193).

## Goal

- Make the map's "camera is moving" state observable in the DOM.
- Wait for that state to settle before clicking a moving popup link.
- Replace the existing URL-stability timing workarounds with the same helper
  where they currently guard map clicks.
- Keep the production behaviour unchanged.
- Fix the e2e stack-start race uncovered while rerunning the suite: Postgres
  reported healthy during its temporary init server, the backend started too
  early and Compose aborted on an unhealthy dependency.

## Implementation

### Map movement signal

In [`BaseMap.tsx`](frontend/src/features/map/BaseMap.tsx:58):

- set `data-map-moving="true"` on the MapLibre container in `onMoveStart`;
- set `data-map-moving="false"` in `onMoveEnd` and once in `onLoad`.

MapLibre emits `movestart` asynchronously, so
[`MapPage.selectStation`](frontend/src/features/map/MapPage.tsx:114) also sets
`data-map-moving="true"` synchronously before calling `flyTo`. Otherwise a test
that checks immediately after the marker click can observe the stale `false`
and click the still-moving popup.

The attribute is a small, test-observable signal analogous to the marker
`alt`/`title` and cluster `data-count` hooks already used by the e2e suite.

### E2E helper

In [`helpers.ts`](frontend/e2e/helpers.ts:85) add:

```ts
export async function waitForMapToSettle(page: Page): Promise<void> {
  await expect(page.locator('.maplibregl-map')).toHaveAttribute('data-map-moving', 'false')
}
```

### E2E database healthcheck

In [`docker-compose.yml`](docker-compose.yml:18) force `pg_isready` over TCP
(`-h 127.0.0.1`) instead of the Unix socket. The Postgres docker-entrypoint init
phase runs a temporary server that answers on the socket only; without the TCP
probe, Compose marks the database healthy while the seed is still running and
starts the backend before the real server is up.

### Controlled individual-range inputs

While rerunning the suite, the settings test
[`settings.spec.ts`](frontend/e2e/settings.spec.ts:183) also failed: filling
From and To back to back let the second setter overwrite the first with the
stale default range. [`useTimeframeSettings`](frontend/src/features/settings/useTimeframeSettings.ts:175)
now chains setters through a short-lived pending buffer, so the second call
sees the first call's value even before React re-renders. A unit test covers
two setters invoked from the same render.

### Specs

- [`map.spec.ts`](frontend/e2e/map.spec.ts:17): call `waitForMapToSettle(page)`
  after the popup content assertions and before clicking `Open detail page`.
- [`flags.spec.ts`](frontend/e2e/flags.spec.ts:33) and
  [`detail.spec.ts`](frontend/e2e/detail.spec.ts:193): replace the URL-stability
  poll with `waitForMapToSettle(page)`, which directly waits for `moveend`
  instead of inferring it from an unchanged URL.

## Gate results

- `make check` — passed.
- `make test-unit` — 91 files / 584 tests passed (includes the new controlled
  range regression test).
- `make coverage` — backend and frontend thresholds passed.
- `make test-playwright` — two consecutive runs, 76/76 passed both times.

## Definition of done

- [x] `BaseMap` exposes `data-map-moving` and keeps it accurate
- [x] `MapPage.selectStation` marks the camera as moving before `flyTo`
- [x] `waitForMapToSettle` helper added
- [x] `map.spec.ts`, `flags.spec.ts` and `detail.spec.ts` use the helper
- [x] Postgres healthcheck waits for the real TCP server
- [x] Back-to-back individual From/To edits keep both values
- [x] `make check`, `make coverage`, `make test-unit` and two Playwright runs green
- [x] Plan status updated to `implemented`
