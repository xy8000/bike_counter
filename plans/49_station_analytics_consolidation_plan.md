# 49 - Station analytics consolidation + measurement sum N+1 fix plan

Status: implemented

## Problem

The backend aggregates station data for the BFF in five separate domain modules,
each with its own tiny driving port and application service:

- [`station_summary`](backend/src/core/domain/station_summary/mod.rs:1) (singular) — sidebar/search per-station summaries.
- [`stations_summary`](backend/src/core/domain/stations_summary/mod.rs:1) (plural) — the aggregated summary page.
- [`station_overview`](backend/src/core/domain/station_overview/mod.rs:1) — the overview page metrics.
- [`station_detail`](backend/src/core/domain/station_detail/mod.rs:1) — the detail page graphs.
- [`global_summary`](backend/src/core/domain/global_summary/mod.rs:1) — whole-system header stats.

The singular/plural `station_summary` vs `stations_summary` split is confusing, and
the five services share a lot of copy-pasted logic:

- `weekday_totals` (identical in two services).
- a per-channel `sum` loop (`sum_window`) in four services.
- the four-metric `MetricWindow` computation in two services.
- the `last_update` lookup in three services.
- the day/week/30-days/year window setup in two services.
- the `SECONDS_PER_*` constants in two services.
- the in-memory test repositories reimplemented in five test modules.

There is also an N+1 query pattern: [`MeasurementRepository::sum`](backend/src/core/domain/measurements/repository_port.rs:86)
takes a single `Option<ChannelId>`, so every service issues one query per channel,
while the other aggregate methods already accept `&[ChannelId]` and use `ANY(...)`.

## Goals

- Consolidate the five station modules into one `station_analytics` domain module
  and one application service (`StationAnalyticsService`) with shared private
  helpers. This removes the duplicated logic and the confusing naming, while
  keeping the BFF payloads and frontend behavior identical.
- Replace the per-channel `sum` loop with a single multi-channel query.
- Deduplicate the BFF image-resolution and DTO-conversion boilerplate.
- Collapse the duplicated in-memory test repositories into one shared test double.

## Non-goals

- No frontend changes. BFF request/response shapes stay identical.
- No new features: no date-picker, no Redis/Valkey caching (see [Future seams](#future-seams)).
- No changes to the REST `/api/v1` public API.
- No splitting into more submodules — the opposite.

## Backend changes

### 1. Multi-channel `sum`

Change [`MeasurementRepository::sum`](backend/src/core/domain/measurements/repository_port.rs:86)
from `sum(from, to, channel_id: Option<ChannelId>)` to
`sum(from, to, channel_ids: &[ChannelId])`. The `None` (all channels) form is only
used by tests, so it can be dropped.

- Postgres impl in [`measurement_repository.rs`](backend/src/adapter/driven/postgres/measurement_repository.rs:202)
  switches to `channel_id = ANY($1::uuid[])` (same pattern as `sum_buckets`).
- Update every in-memory mock in the application service tests and the REST
  [`mocks.rs`](backend/src/adapter/driving/rest/tests/mocks.rs:249).

### 2. New `station_analytics` domain module

Create `core/domain/station_analytics/mod.rs` holding all the models that today
live in the five modules, and `core/domain/station_analytics/service_port.rs` with
one driving port:

```rust
pub trait StationAnalyticsServicePort: Send + Sync {
    fn summaries(&self, bounds: Option<GeoBounds>, now: DateTime<Utc>) -> Result<Vec<StationSummary>, DomainError>;
    fn global_summary(&self, now: DateTime<Utc>) -> Result<GlobalSummary, DomainError>;
    fn overview(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationOverview, DomainError>;
    fn detail(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationDetail, DomainError>;
    fn stations_summary(&self, bounds: GeoBounds, exclude: &[Id], now: DateTime<Utc>) -> Result<StationsSummary, DomainError>;
}
```

Move these types unchanged: `StationSummary`, `StationsSummary`,
`SummaryStation`, `StationsSummaryGraphs`, `SummaryPeriodGraphs`,
`StationTotal`, `PerStationSeries`, `StationOverview`, `MetricWindow`,
`MetricKey`, `StationDetail`, `StationDetailGraphs`, `PeriodGraphs`,
`PerChannelSeries`, `GlobalSummary`, and `bounds::GeoBounds`.

Delete the five old modules and their `pub mod` lines in
[`core/domain/mod.rs`](backend/src/core/domain/mod.rs:1).

### 3. Single `StationAnalyticsService`

Create `core/application/station_analytics_service.rs` with one struct that holds
the same four repositories (counting station, channel, measurement, job) and
implements the five methods. Extract the shared private helpers:

- `sum_window(from, to, channel_ids)` — one `sum` call (no loop) after step 1.
- `weekday_totals(buckets, tz)`.
- `metric_windows(...)` — the four `MetricWindow` values for one station (reused
  by `overview` and, in a loop, by `stations_summary`).
- `period_graphs(...)` — the shared bucket/radar aggregation; parameterized so the
  per-channel variant (detail) and per-station variant (summary) share the heavy
  `sum_buckets_by_channel`/`sum_hours_by_channel` reads.
- `last_update()` — one `find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)`.
- the day/week/30-days/year window computation.

Delete the five old application service files and their `pub mod` lines in
[`core/application/mod.rs`](backend/src/core/application/mod.rs:1).

### 4. Tests

Move the five test modules into one `#[cfg(test)]` module under
`station_analytics_service.rs`. Introduce one shared in-memory test double for
each repository (counting station, channel, measurement, job) instead of the
current per-file copies. Keep all existing assertions; only the wiring changes.

### 5. Wiring and BFF handlers

- [`main.rs`](backend/src/main.rs:215) builds one `StationAnalyticsService` instead
  of five.
- [`AppState`](backend/src/adapter/driving/rest/handlers/mod.rs:49) replaces the
  five `Arc<dyn *ServicePort>` fields with one
  `Arc<dyn StationAnalyticsServicePort>`.
- [`bff/handlers.rs`](backend/src/adapter/driving/bff/handlers.rs:1) calls the new
  service methods and extracts a shared image-resolution helper (used by the
  overview and detail handlers, which currently duplicate it).

### 6. BFF DTO dedup

In [`bff/dto.rs`](backend/src/adapter/driving/bff/dto.rs:299) the
`StationDetailGraphsDto` and `StationsSummaryGraphsDto` conversions duplicate the
`buckets`/`weekdays`/`hours`/`monthly` closures. Hoist those into shared free
functions.

## Future seams

- **Date picker**: the service methods already take `now: DateTime<Utc>`; the
  window helpers become the single place to swap "previous complete period" for
  an arbitrary selected range. No change now.
- **Redis/Valkey cache**: the BFF handlers remain thin and the DTOs are already
  `Serialize`/`Deserialize`, so a cache can be added at the handler layer without
  touching the domain.

## Definition of done

- [x] `station_analytics` module + single service replace the five modules/services.
- [x] `MeasurementRepository::sum` takes `&[ChannelId]`; no per-channel loops remain.
- [x] BFF payloads unchanged (frontend untouched, `make test-rest` green).
- [x] `make check`, `make test-rest`, `make test`, `make coverage` green.
