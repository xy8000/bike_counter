# 151 - Fix open Sonar findings and re-target the pending release to v0.0.6

Status: implemented

## Problem

SonarCloud reports a batch of open findings across the frontend (one High
security pair plus a set of maintainability smells). Separately, the prepared
`v0.0.7` tag was deleted, so the still-pending release (plan
[150](150_release_v0.0.6_plan.md)) is re-targeted to `v0.0.6`.

## Findings

| # | File | Finding | Severity | Fix |
|---|---|---|---|---|
| 1 | [`frontend/src/lib/bff.ts`](../frontend/src/lib/bff.ts:12) | Client-Side Request Forgery via unsanitized user input | High | validate the request URL inside the shared `getJson` sink |
| 2 | [`frontend/src/lib/bff.ts`](../frontend/src/lib/bff.ts:12) | Server-Side Request Forgery via unsanitized user input | Medium | same as #1 |
| 3 | [`frontend/e2e/helpers.ts`](../frontend/e2e/helpers.ts:100) | Async function `readTheme` has no `await` expression | Low | drop `async`, return the promise directly |
| 4 | [`frontend/src/features/map/MapView.tsx`](../frontend/src/features/map/MapView.tsx:46) | Cognitive complexity 16 > 15 | High | extract the popup ternaries into small components |
| 5-8 | [`frontend/src/features/map/MapView.tsx`](../frontend/src/features/map/MapView.tsx:69) | Nested ternary (×4: L69, L94, L96, L107) | Medium | same extraction as #4 |
| 9 | [`frontend/src/features/stationOverview/useStationOverview.ts`](../frontend/src/features/stationOverview/useStationOverview.ts:33) | Avoid nesting promises | Medium | rewrite the effect body with `async`/`await` |
| 10 | [`frontend/src/features/stationOverview/useStationOverview.ts`](../frontend/src/features/stationOverview/useStationOverview.ts:36) | Avoid nesting promises | Medium | same as #9 |
| 11-18 | [`frontend/scripts/render-check.mjs`](../frontend/scripts/render-check.mjs:17) | `await` inside a loop (×8) | Low | drive the scenarios through `Promise.all(scenarios.map(async …))` |
| 19 | [`frontend/src/features/stationsSummary/SummaryMap.tsx`](../frontend/src/features/stationsSummary/SummaryMap.tsx:11) | Mark the props of the component as read-only | Low | wrap the props type in `Readonly<…>` |
| 20-22 | [`frontend/vitest.setup.tsx`](../frontend/vitest.setup.tsx:86) | Mark the props of the component as read-only (×3) | Low | wrap the mock props types in `Readonly<…>` |
| 23-25 | [`frontend/vitest.setup.tsx`](../frontend/vitest.setup.tsx:158) | Unexpected empty class (×3) | Low | replace the empty `class {}` mocks with one non-empty mock class |

## Approach

### 1. BFF request guard (findings 1-2)

[`getJson()`](../frontend/src/lib/bff.ts:11) is the single fetch sink every
feature `api.ts` shares, and its `url` argument is not validated there — a
server-provided HATEOAS `_links.*.href` therefore reached `fetch` unguarded in
the feature modules that do not pre-validate (the stations API already ran its
URLs through `assertSafeBffUrl`, which is why the finding moved here).

- New [`assertSafeRequestUrl()`](../frontend/src/lib/apiUrl.ts) in
  [`apiUrl.ts`](../frontend/src/lib/apiUrl.ts): returns the URL only when it is a
  root-relative, same-origin path (starting with a single `/`), rejecting
  absolute URLs (`https://…`), protocol-relative URLs (`//host`),
  backslash-smuggled paths (`/\host`) and control characters — and throws
  otherwise.
- [`getJson()`](../frontend/src/lib/bff.ts:11) now fetches
  `assertSafeRequestUrl(url)` so *every* caller is sanitized at the sink.
- The stricter BFF-only allowlist ([`assertSafeBffUrl()`](../frontend/src/lib/apiUrl.ts))
  stays in place for the stations API, which is the one module whose request URL
  can come straight from a server link.

### 2. `readTheme` (finding 3)

[`readTheme()`](../frontend/e2e/helpers.ts:100) only returns `page.evaluate(…)`
and never awaits, so it drops the `async` keyword and keeps its
`Promise<…>` return type (callers still `await` it).

### 3. MapView popup (findings 4-8)

The popup body's loading-state rendering is a chain of nested ternaries
(icon, channel badge, description), which is what pushes its cognitive
complexity to 16. Each ternary becomes a tiny component
(`StationPopupIcon`, `StationPopupBadge`, `StationPopupDescription`) that uses
early returns, so the body has no nested ternaries and is well under the
complexity budget.

### 4. Station-overview promises (findings 9-10)

The `.then(... then fetchStationOverviewStats(...))` nesting in
[`useStationOverview.ts`](../frontend/src/features/stationOverview/useStationOverview.ts:27)
is flattened into an `async` loader called from the effect, with the shell and
the stats each in their own `try`/`catch` so they still fail independently.

### 5. `render-check.mjs` (findings 11-18)

The debug script drives its scenarios with `await` inside a `for…of`. The
per-scenario body moves into an `async` callback run through
`Promise.all(scenarios.map(…))`, so no `await` sits directly inside a loop.

### 6. Read-only props + empty classes (findings 19-25)

- [`SummaryMap`](../frontend/src/features/stationsSummary/SummaryMap.tsx:11) and
  the three vitest map mocks wrap their props types in `Readonly<…>`, matching
  the convention already used by [`MapView`](../frontend/src/features/map/MapView.tsx:135).
- The three empty `class {}` mocks in
  [`vitest.setup.tsx`](../frontend/vitest.setup.tsx:158) become one small
  non-empty `FakeMaplibreClass`.

### 7. Re-target the release to v0.0.6

The `v0.0.7` tag was deleted, so the pending release (plan
[150](150_release_v0.0.6_plan.md)) is re-targeted to `v0.0.6`:

- `0.0.7` → `0.0.6` in [`backend/Cargo.toml`](../backend/Cargo.toml:3),
  [`backend/Cargo.lock`](../backend/Cargo.lock:481),
  [`frontend/package.json`](../frontend/package.json:4),
  [`frontend/package-lock.json`](../frontend/package-lock.json:3) (two fields),
  [`docker-compose.yml`](../docker-compose.yml:29) (both image tags),
  [`README.md`](../README.md:84) and the
  [`release.yml`](../.github/workflows/release.yml:9) doc-comment examples.
- Plan [150](150_release_v0.0.6_plan.md) is retitled/renamed to v0.0.6.

## Verification

- `npx prettier --check .` — clean.
- `npx tsc --noEmit` — clean.
- `npm run test:unit` — green (the request guard keeps the existing
  root-relative fixtures valid).
- `make check` — green (fmt + clippy + prettier + cargo audit).

## Definition of done

- [x] Plan file added
- [x] `getJson` validates its URL; `assertSafeRequestUrl` added + tested
- [x] `readTheme` no longer `async`
- [x] MapView popup ternaries extracted; cognitive complexity < 15
- [x] `useStationOverview` effect has no nested promises
- [x] `render-check.mjs` has no `await` inside a loop
- [x] `SummaryMap` + vitest mock props are read-only; no empty classes
- [x] Release re-targeted to `v0.0.6` (version files, compose, README, release.yml, plan 150)
- [x] `make check` green; `npm run test:unit` green (88 files / 561 tests)
- [x] `make test-rest` green (128 passed)
- [x] Branch pushed
