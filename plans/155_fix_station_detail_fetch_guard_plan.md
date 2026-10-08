# 155 - Guard the station-detail `useResource` fetch (Sonar CSRF/SSRF)

Status: implemented

## Problem

SonarCloud reports two open security findings:

- **Client-Side Request Forgery via unsanitized user input** — Security, 4 High (L17)
- **Server-Side Request Forgery via unsanitized user input** — Security, 2 Medium (L17)

These are the same rule pair that plan [132](132_sonar_security_findings_fix_plan.md)
fixed for [`frontend/src/features/stations/api.ts`](../frontend/src/features/stations/api.ts)
and plan [151](151_sonar_findings_and_v0.0.6_plan.md) chased into the shared
[`getJson()`](../frontend/src/lib/bff.ts:16) sink. One raw sink was missed.

## Root cause

Every feature API routes its request through the shared
[`getJson()`](../frontend/src/lib/bff.ts:16), which validates the URL with
[`assertSafeRequestUrl()`](../frontend/src/lib/apiUrl.ts:12) before `fetch`. The
station-detail cards instead load through the
[`useResource()`](../frontend/src/features/stationDetail/useResource.ts:6) hook,
which calls `fetch` **directly** at
[`useResource.ts:17`](../frontend/src/features/stationDetail/useResource.ts:17)
without validating its `url` argument — the L17 sink Sonar flags.

The `url` is never a constant: the three callers hand it a server-provided
HATEOAS `_links.*.href`
([`useStationOverviewStats`](../frontend/src/features/stationDetail/useStationOverviewStats.ts:8),
[`useStationGraphs`](../frontend/src/features/stationDetail/useStationGraphs.ts:9),
[`useStationMonthly`](../frontend/src/features/stationDetail/useStationMonthly.ts:10)),
so a compromised or spoofed response could point the browser at an arbitrary
cross-origin URL (the SSRF/CSRF gadget).

## Fix

Route the hook through the same validated sink as everything else: replace the
raw `fetch(url)` in
[`useResource()`](../frontend/src/features/stationDetail/useResource.ts:17) with
the shared [`getJson<T>(url)`](../frontend/src/lib/bff.ts:16), which sanitizes
the URL as a root-relative, same-origin path
([`assertSafeRequestUrl`](../frontend/src/lib/apiUrl.ts:12)) before fetching.

- No behaviour change for legitimate (root-relative) card links: the hook keeps
  its `loading`/`data`/`error` state, the stale-response/unmount cancellation
  guards and its "no fetch when `url` is null" contract.
- An off-origin / malformed URL now rejects inside `getJson` and surfaces as the
  hook's `error` state **without** ever calling `fetch`.
- Existing [`useResource.test.tsx`](../frontend/src/features/stationDetail/useResource.test.tsx)
  mocks `fetch` and asserts the URL string, so it stays valid; a new regression
  test asserts an off-origin URL is rejected before `fetch`.

## Definition of done

- [x] Plan file in [`plans/`](.) created
- [x] `useResource` fetches through the validated shared `getJson`
- [x] New regression test: off-origin URL sets `error` and never calls `fetch`
- [x] `npm run test:unit` green (88 files / 562 tests; hook suite 8 tests)
- [x] `make check` green (fmt + clippy + prettier + cargo audit)
- [x] `make test-rest` green (128 passed)
