# 50 - Station analytics service split plan

Status: implemented

## Problem

Plan 49 consolidated five station analytics services into one
`StationAnalyticsService`, but the single file
[`station_analytics_service.rs`](backend/src/core/application/station_analytics_service.rs:1)
has grown to ~2400 lines (roughly 970 production + ~1400 test). It now mixes
three distinct concerns:

- **Orchestration** — fetching stations/channels and assembling the five BFF
  payloads (`summaries`, `global_summary`, `overview`, `detail`,
  `stations_summary`).
- **The four overview metrics** — the `MetricWindow` computation.
- **The graph/window bucketing** — the `Window`/`GraphWindows`/`period_data`
  machinery shared by the detail and summary pages.

The graph/window code is also the future date-picker seam, but today it is
tangled up with the orchestration, which makes both hard to change in isolation.

## Goals

- Split the application service into a `station_analytics/` module folder with
  three cohesive files: `service.rs` (orchestration), `metrics.rs` (the four
  overview metrics) and `graphs.rs` (the window/bucket aggregation).
- Turn the helpers into **free functions** that take the repositories they need
  as parameters (instead of methods on the whole service), so they are
  independently unit-testable and no longer reach into the full service state.
  This is a real re-shaping, not a line-by-line file move.
- Keep the single public driving port `StationAnalyticsServicePort` — **no new
  internal traits**.
- Keep all BFF payloads and the frontend behavior byte-for-byte identical.

## Non-goals

- No new internal traits (the user explicitly chose free/associated functions).
- No date-picker and no Redis/Valkey cache implementation (see
  [Future seams](#future-seams)).
- No frontend changes, no REST/BFF API changes.
- No changes to the domain `station_analytics` module (models + port stay as-is).
- No splitting of the test module into many tiny per-test files.

## Backend changes

### 1. New module folder

Replace `core/application/station_analytics_service.rs` with
`core/application/station_analytics/`:

- `mod.rs` — declares `graphs`, `metrics`, `service` and
  `#[cfg(test)] mod tests`, then re-exports `StationAnalyticsService`.
- `service.rs` — the struct, `new`, and the `StationAnalyticsServicePort` impl
  (the five methods become thin orchestrators that delegate to the helpers).
- `metrics.rs` — `sum_window`, `last_update`, `metric_windows`.
- `graphs.rs` — `Window`/`WindowPair`/`GraphWindows`/`PeriodData`/
  `GroupGraphData`, `weekday_totals`, `graph_windows`, `period_data`,
  `period_graphs_per_channel`, `period_graphs_per_station`,
  `stations_summary_graphs`, `empty_graphs`, and the `SECONDS_PER_*` constants.
- `tests.rs` — the existing `#[cfg(test)] mod tests` moved wholesale with only
  the imports adapted.

Update [`core/application/mod.rs`](backend/src/core/application/mod.rs:13) from
`pub mod station_analytics_service;` to `pub mod station_analytics;`.

### 2. Orchestration stays in `service.rs`

[`service.rs`](backend/src/core/application/station_analytics/service.rs) keeps
`StationAnalyticsService` and its four repository fields, plus the private
`channels_by_station` helper (returns `HashMap<Uuid, Vec<Channel>>`, used only
by `stations_summary`). The five port methods keep their current behavior but
call into `metrics::` and `graphs::` free functions, passing
`self.measurement_repository.as_ref()` (and friends) as arguments.

### 3. Metric computation in `metrics.rs`

[`metrics.rs`](backend/src/core/application/station_analytics/metrics.rs) owns:

- `sum_window(repository, from, to, channel_ids)` — one multi-channel `sum`.
- `last_update(job_repository)` — newest finished data-source-update job.
- `metric_windows(repository, stations, channels_by_station, now)` — the four
  `MetricWindow` values, keeping the existing `HashMap<Uuid, Vec<Channel>>`
  map signature so no extra id maps are introduced.

### 4. Graph computation in `graphs.rs`

[`graphs.rs`](backend/src/core/application/station_analytics/graphs.rs) owns the
window/bucket machinery as free functions. `period_data` remains the single
shared heavy aggregation; `period_graphs_per_channel` and
`period_graphs_per_station` parameterize it, and `stations_summary_graphs`
composes the four timeframes plus the monthly totals. All take
`&dyn MeasurementRepository`.

### 5. Import-path updates

`StationAnalyticsService` moves to `core::application::station_analytics`. Update
the four import sites:

- [`main.rs`](backend/src/main.rs:32)
- [`rest/tests/mocks.rs`](backend/src/adapter/driving/rest/tests/mocks.rs:23)
- [`rest/tests/bff.rs`](backend/src/adapter/driving/rest/tests/bff.rs:20)
- [`rest/tests/mod.rs`](backend/src/adapter/driving/rest/tests/mod.rs:41)

### 6. Tests

Move the `#[cfg(test)] mod tests` into `station_analytics/tests.rs` with all
assertions unchanged. Adapt `use super::*` to import from
`super::{graphs, metrics, service}` and explicitly import
`DATA_SOURCE_UPDATE_JOB_TYPE` and the `StationAnalyticsServicePort` trait, which
were previously pulled in implicitly via `use super::*`.

## Future seams

- **Date picker**: all window math now lives in two places — `graphs.rs`
  (`graph_windows`) and `metrics.rs` (`metric_windows`) — both driven by
  `now: DateTime<Utc>`. A future selected range (`TimeRange`/`from`/`to`)
  replaces `now` in exactly these two functions, and `service.rs` passes the
  range through without touching the aggregation.
- **Redis/Valkey cache**: the BFF handlers already depend on
  `Arc<dyn StationAnalyticsServicePort>`; a caching decorator implementing that
  trait can be introduced in the BFF adapter layer without touching the domain or
  the service split.

## Definition of done

- [x] `station_analytics/` folder with `mod.rs`, `service.rs`, `metrics.rs`,
      `graphs.rs`, `tests.rs` replaces `station_analytics_service.rs`.
- [x] No new traits; `StationAnalyticsServicePort` remains the single port.
- [x] Import sites updated; `make check` and `make test-rest` green.
- [x] `make test` and `make coverage` green (core >= 95%).
- [x] Plan registered in [`plans/README.md`](plans/README.md).
