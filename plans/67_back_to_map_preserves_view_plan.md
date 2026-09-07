# 67 - Back to map preserves the previous view

Status: implemented

## Problem

Going "Back to map" from the station-summary page does not return to the view the
user had before. Both detail pages render a plain `<Link to="/">`, so the map
re-seeds its bounds from an empty URL and starts at the Münster default instead
of the previously visible area. The existing e2e assertions pass only because the
map's own `moveend` sync writes fresh default bounds back into the URL.

Affected links:

- [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:247)
- [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:194)

## Root cause

The map view lives in the URL (`min_lat` / `min_lng` / `max_lat` / `max_lng`, see
plan 34). The summary route already carries those bounds (plan 41 passes them from
the map), but the "Back to map" link drops them. The detail route does not carry
bounds at all (navigated with `navigate(/stations/:id)`), so it has nothing to
restore from its own URL.

## Scope

Frontend-only. No backend, API or data-model changes.

## Decisions

- **Summary page**: rebuild the target from the bounds already present in the
  `/summary` URL. Deterministic, works for shared links, and keeps the map as the
  single source of truth.
- **Detail page**: use the browser history to restore the exact prior view (the
  previous entry is the map with its bounds and open station). Keep a `<Link>` for
  the role/`href` (a11y + existing locators), but intercept the click with
  `preventDefault()` + `navigate(-1)` only when the page was reached via in-app
  navigation (`location.key !== 'default'`). Shared/deep links (direct load, key
  `'default'`) fall back to `/`.
- Documented edge case: `map -> summary -> detail -> Back to map` would
  history-back to `/summary` rather than `/`. Accepted for now; threading bounds
  into the detail route is the alternative if "always the map" becomes a
  requirement.

## Changes

### 1. [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:247)

- Replace `<Link to="/">` with a target built from the already-parsed `bounds`:
  `to={bounds ? `/?${serializeBounds(bounds)}` : '/'}`.
- Keep the `Button asChild variant="outline" size="sm"` wrapper and the
  `<ArrowLeft /> Back to map` content, so the accessible name and styling are
  unchanged.

### 2. [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:194)

- Import `useLocation` and `useNavigate` (already imported `useNavigate`).
- Add `const location = useLocation()` and
  `const hasInAppHistory = location.key !== 'default'`.
- Replace `<Link to="/">` with a link whose `to` is the `/` fallback and whose
  `onClick` calls `event.preventDefault()` + `navigate(-1)` when
  `hasInAppHistory` is true. Middle-click/`target` behavior keeps the `href="/"`
  fallback.

### 3. e2e

- [`summary.spec.ts`](../frontend/e2e/summary.spec.ts:118): strengthen the
  back-to-map assertion to check that the restored URL carries the **same** bbox
  values as the `/summary` URL it came from (not merely that some `min_lat`
  exists).
- [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:111) / `map.spec.ts`: keep the
  back-to-map tests green; assert the in-app navigation (marker -> detail ->
  back) returns to the map with the previous bounds.

## Gates

- `npm run build` (tsc + vite).
- `make test-playwright` (frontend UI changed).
- Backend untouched, so `make check` / `make test-rest` are expected to stay
  green and re-run to be safe.

## Implementation notes

- [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:247)
  now renders `to={bounds ? \`/?${serializeBounds(bounds).toString()}\` : '/'}` so
  the map re-seeds from the `/summary` URL bounds.
- [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:202)
  reads `location.key`; when `!== 'default'` (in-app navigation) the Back-to-map
  link calls `preventDefault()` + `navigate(-1)` and otherwise keeps `href="/"`.
- e2e: [`summary.spec.ts`](../frontend/e2e/summary.spec.ts:118) compares the
  restored bbox against the `/summary` URL bbox with `toBeCloseTo(…, 6)`;
  [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:128) adds the in-app
  back-to-map test.
- Note: `npm run build` + `make test-playwright` (23/23) green.

## Definition of done

- [x] Summary "Back to map" restores the bounds from the `/summary` URL.
- [x] Detail "Back to map" restores the prior map view via history, with a `/`
      fallback for deep links.
- [x] e2e updated and `make test-playwright` green.
