# 98 - Bike-Trends resolution changer (high / mid / low)

Status: implemented (make check + make test-rest + make coverage + make test-playwright green)

## Problem

The Bike-Trends settings dialog lets the user pick the period (day / week /
last 30 days / year / individual range), compare the previous period and exclude
new stations — but the chart bucket resolution is fixed:

- day → 5-minute buckets
- week → 1-hour buckets
- last 30 days → 1-day buckets
- year → 1-day buckets
- individual → derived from the range length (15m / hour / day / week / month / quarter)

The request is a **resolution changer** in the same dialog: three levels
`high` / `mid` / `low`, rendered exactly like the Period buttons (one-click
choices that also show the concrete bucket size).

## Goal / Decisions

1. One persisted `resolution` setting with three levels: `high`, `mid`, `low`
   (confirmed: the **default is `mid`**).
2. The concrete bucket sizes per fixed timeframe:

   | Timeframe     | high    | mid    | low   |
   |---------------|---------|--------|-------|
   | day (24 h)    | 15 Min  | 30 Min | Hour  |
   | week          | 30 Min  | Hour   | Day   |
   | last 30 days  | Hour    | Day    | Week  |
   | year          | Day     | Week   | Month |

3. The `day` timeframe's previous 5-minute default is **replaced** by 15 minutes
   (the finest offered); there is no 5-minute option anymore.
4. **Individual** ranges reuse the closest fixed timeframe's set, matched by
   span (confirmed):

   - span ≤ 2 days   → day set
   - span ≤ 7 days   → week set
   - span ≤ 45 days  → last-30-days set
   - span ≤ 1 year   → year set
   - span > 1 year   → `[Week, Month, Quarter]` (the `Day` option is dropped,
     `Quarter` fills the low slot)

   Changing the from/to range therefore re-labels the three buttons; the chosen
   level (`high`/`mid`/`low`) stays and maps to the new concrete size.
5. The resolution is shared by the detail page and the station-summary page and
   is persisted like the other timeframe settings: URL (primary, shareable) +
   cookie (bare-URL fallback).
6. The frontend computes the concrete granularity and sends it as a
   `resolution` query parameter on the graph sub-resource URLs; the backend maps
   it to a bucket granularity. When the parameter is absent the backend keeps
   its current behavior, so existing callers are unaffected.

## Backend granularity tokens

The frontend sends one of `15m | 30m | hour | day | week | month | quarter`.
The backend maps it to `BucketGranularity`:

| token    | `BucketGranularity`              |
|----------|----------------------------------|
| `15m`    | `Fixed { seconds: 900 }`         |
| `30m`    | `Fixed { seconds: 1800 }`        |
| `hour`   | `Fixed { seconds: 3600 }`        |
| `day`    | `Day` (calendar day)             |
| `week`   | `Week` (calendar ISO week)       |
| `month`  | `Month` (calendar month)         |
| `quarter`| `Quarter` (calendar quarter)     |

Notes:

- The fixed timeframes currently use `Fixed { seconds: 86400 }` for their day
  buckets. The override uses calendar `Day` instead — correct across DST and
  consistent with the custom-range path. The existing no-parameter behavior is
  untouched.
- Fixed-width origins are already local midnights (`day_from`, `week_start`,
  `last_30_from`, `year_start`), so overriding `day`/`week`/`30 days`/`year`
  with `15m`/`30m`/`hour` keeps hour/quarter-hour alignment without extra work.

## Approach

### Task 1 — Backend domain: `GraphResolution`

In
[`backend/src/core/domain/station_analytics/mod.rs`](../backend/src/core/domain/station_analytics/mod.rs:212):

- Add a `GraphResolution` enum (`FifteenMinutes`, `ThirtyMinutes`, `Hour`, `Day`,
  `Week`, `Month`, `Quarter`) with `as_str()` / `from_key()` and a
  `to_bucket_granularity()` (or `seconds()` + calendar variant) helper.
- Re-export it so the service port and BFF can use it.

### Task 2 — Backend graphs: resolution override

In
[`backend/src/core/application/station_analytics/graphs.rs`](../backend/src/core/application/station_analytics/graphs.rs:89):

- Add a helper that takes a `&mut Window` and a `GraphResolution` and overwrites
  `Window::granularity` (and, for fixed-width granularities, re-derives the
  `origin` if it is not already a local midnight — current origins already are).
- `graph_windows` and `custom_window` stay unchanged; the override is applied by
  the callers (Task 3) after building the windows.

### Task 3 — Backend service + port threading

- Extend the four port methods in
  [`backend/src/core/domain/station_analytics/service_port.rs`](../backend/src/core/domain/station_analytics/service_port.rs:83)
  with a `resolution: Option<GraphResolution>` parameter:
  `detail_graphs_timeframe`, `detail_graphs_custom`,
  `stations_summary_graphs_timeframe`, `stations_summary_graphs_custom`.
- Implement in
  [`backend/src/core/application/station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:543):
  after selecting `pair` / building `current`, apply the override to both
  windows of a fixed timeframe and to the single custom window.

### Task 4 — Backend BFF DTO + handlers

- Add `#[serde(default)] pub resolution: Option<GraphResolution>` to
  [`AsOfQueryParams`](../backend/src/adapter/driving/bff/dto.rs:329) and
  [`BffStationSummaryQueryParams`](../backend/src/adapter/driving/bff/dto.rs:589).
- In
  [`backend/src/adapter/driving/bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:609),
  read `params.resolution` and pass it into the four service calls (both detail
  and summary, fixed and custom). Update the `utoipa::path` param docs.
- `GraphResolution::from_key` validation: an unknown token is rejected with
  `DomainError::InvalidQuery` (400) or treated as `None` — decide and document;
  prefer rejecting so a typo cannot silently change resolution.

### Task 5 — Backend tests

- Unit-test `GraphResolution::from_key` / `to_bucket_granularity` in
  [`mod.rs`](../backend/src/core/domain/station_analytics/mod.rs:384).
- Add a graphs test that applying a resolution to a window changes its
  granularity (e.g. year + `Month` → `BucketGranularity::Month`, day + `30m` →
  `Fixed { seconds: 1800 }`).
- Keep the existing no-parameter behavior green (existing tests must pass
  unchanged).

### Task 6 — Frontend resolution module

New file
[`frontend/src/features/stationDetail/resolution.ts`](../frontend/src/features/stationDetail/resolution.ts:1):

- `type ResolutionLevel = 'high' | 'mid' | 'low'`
- `type GranularityKey = '15m' | '30m' | 'hour' | 'day' | 'week' | 'month' | 'quarter'`
- Per-timeframe option table (the three labels + granularity keys above).
- `resolutionOptions(timeframe, from, to)` returning the three options for a
  fixed timeframe or, for `individual`, the span-matched set (with `> 1 year`
  dropping `day` and appending `quarter`).
- `granularityKey(level, timeframe, from, to)` for the query parameter.
- A label formatter matching the current subtitle wording (`15-minute buckets`,
  `30-minute buckets`, `1-hour buckets`, `1-day buckets`, `1-week buckets`,
  `1-month buckets`, `1-quarter buckets`).

### Task 7 — Frontend settings state

In
[`frontend/src/features/settings/useTimeframeSettings.ts`](../frontend/src/features/settings/useTimeframeSettings.ts:19):

- Add `resolution: ResolutionLevel` to `PersistedTimeframeSettings` (default
  `'mid'`), the URL parsing/apply helpers (`resolution=high|mid|low`) and the
  cookie read/write.
- Add `resolution` + `setResolution` to `TimeframeSettingsValue`.

### Task 8 — Frontend dialog UI

In
[`frontend/src/features/settings/SettingsDialog.tsx`](../frontend/src/features/settings/SettingsDialog.tsx:66):

- Add a "Resolution" block under the Period block that renders the three buttons
  with the same button classes, `aria-pressed` and click handler used by the
  Period buttons. Labels come from `resolutionOptions(...)`.
- Recompute the labels when `timeframe` / `from` / `to` change (individual
  ranges re-label the buttons).

### Task 9 — Frontend resolution-aware chart config

In
[`frontend/src/features/stationDetail/timeframes.ts`](../frontend/src/features/stationDetail/timeframes.ts:76):

- Parameterize `TimeframeConfig` (subtitle, axis, tooltip) by the chosen
  granularity. Add a `timeframeConfig(timeframe, granularity)` (or extend the
  existing config lookup) so the detail and summary pages render the correct
  subtitle/axis/tooltip for the selected resolution.
- Wire it in
  [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:261)
  and
  [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:323):
  compute `cfg` from the resolution key and pass it to the existing chart code
  (the axis formatters already cover hour/day/week/month/quarter; add a
  30-minute/hour label path and a weekday-only axis for the week-day resolution
  if needed).

### Task 10 — Frontend graph URL

- Extend the graph-fetch helpers
  ([`api.ts`](../frontend/src/features/stationDetail/api.ts:37) and
  [`stationsSummary/api.ts`](../frontend/src/features/stationsSummary/api.ts:41))
  to append `resolution=<granularity-key>` (reuse the existing `withTrendParam`
  / `withSummaryParams` pattern).
- In `StationDetail.tsx` / `StationsSummary.tsx`, build the graph link including
  the resolution param for both fixed and individual timeframes.

### Task 11 — Playwright e2e

- Add a spec (or extend
  [`frontend/e2e/settings.spec.ts`](../frontend/e2e/settings.spec.ts:1)) that
  opens the dialog, switches the resolution and asserts:
  - the three buttons render with the timeframe-dependent labels,
  - the graph request carries `resolution=<expected-key>`,
  - the value persists in the URL and is restored on a bare URL.

### Task 12 — Plan + docs

- Write this plan file and register it in
  [`plans/README.md`](../plans/README.md:13).
- Update [`README.md`](../README.md) / `ToDo.md` as the agents workflow requires
  (settings documentation).

## Definition of done

- [x] `make check` green
- [x] `make test-rest` green
- [x] `make coverage` green
- [x] `make test-playwright` green (frontend UI touched)
