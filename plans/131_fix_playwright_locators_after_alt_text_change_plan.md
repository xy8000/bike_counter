# 131 - Fix Playwright locators after the Sonar alt-text changes

Status: implemented

## Problem

Commit `a548314` ([plan 129](129_sonar_active_findings_plan.md)) removed the
redundant `image`/`icon` suffixes from the frontend `<img>` alt texts (the Sonar
rule that alt text should not contain words like "image"). The Playwright e2e
specs were not updated, so `make test-playwright` fails on `main` (67 passed /
7 failed) — the failures are assertion-level, not infrastructure:

- [`frontend/e2e/data-sources.spec.ts`](../frontend/e2e/data-sources.spec.ts:39)
  still queries `getByAltText('Münster image')`; the detail image is now
  `alt="Münster"` ([`DataSourceDetail.tsx`](../frontend/src/features/dataSources/DataSourceDetail.tsx:171)).
- [`frontend/e2e/summary.spec.ts`](../frontend/e2e/summary.spec.ts:85) still
  queries `getByRole('img', { name: 'Station summary image' })`; the summary
  image is now `alt="Station summary"`
  ([`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:375)).
- [`frontend/e2e/map.spec.ts`](../frontend/e2e/map.spec.ts:86) still queries
  `overview.getByAltText(\`${stationName} image\`)`; the overview banner image
  is now `alt={stationName}`
  ([`StationOverview.tsx`](../frontend/src/features/stationOverview/StationOverview.tsx:106)).
- [`frontend/e2e/flags.spec.ts`](../frontend/e2e/flags.spec.ts:29) uses
  `getByAltText(stationName, { exact: true })` to target the map flag. Because
  the overview banner image now also carries `alt={stationName}`, the locator
  matches two elements once the overview is open → Playwright strict-mode
  violation (tests 1, 2, 3 and 5).

## Change

- Add a `stationMarker(page, name)` helper to
  [`frontend/e2e/helpers.ts`](../frontend/e2e/helpers.ts) that scopes to the
  `.station-marker` class rendered by `stationMarkerImage`
  ([`frontend/src/lib/map.tsx`](../frontend/src/lib/map.tsx:46)) — the station
  name alone is no longer unique across the page.
- Use it in [`flags.spec.ts`](../frontend/e2e/flags.spec.ts) instead of the bare
  `getByAltText`.
- Update the three stale alt-text locators in `data-sources.spec.ts`,
  `summary.spec.ts` and `map.spec.ts` to the new alt text.

No production code changes; only the e2e specs and their shared helper.

## Resolution

All four affected specs were updated to the new alt texts, and
`flags.spec.ts` now targets the map flag through the shared
`stationMarker(page, name)` helper. No production code changed.

`make test-playwright` is green (74 passed), up from 67 passed / 7 failed before
the fix.

## Definition of done

- [x] `stationMarker` helper added to `frontend/e2e/helpers.ts`
- [x] `flags.spec.ts` uses the helper for every map-flag locator
- [x] `data-sources.spec.ts`, `summary.spec.ts`, `map.spec.ts` alt locators updated
- [x] `make test-playwright` green (74 passed)
- [x] Committed
