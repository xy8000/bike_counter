# 164 - Remove the crate-wide `allow(dead_code)` and the dead code it hides
Status: implemented

## Context

[`backend/src/main.rs`](backend/src/main.rs) started with `#![allow(dead_code)]`.
That is an inner attribute at the crate root of a binary-only crate (there is no
`lib.rs`), so it silenced dead-code analysis for the **whole** backend — `mod
core`, `mod adapter` and `fn main` alike. It was added by the initial domain
scaffolding commit (`bf9be48`, 2026-08-24) to keep
`cargo clippy --all-targets -- -D warnings` green while the port surface was
written ahead of its callers: removing the line produced **55 dead-code errors
in the bin target and 28 in the test target**.

The blanket allow also hid genuine rot (obsolete items no longer warned) and
blocked the `#[expect(dead_code)]` workflow. This plan removes the attribute and
the dead code behind it, so the normal `-D warnings` bar applies again.

## Policy (as implemented)

1. **Used nowhere (not even by tests)** → delete the item, its construction
   sites and unused imports.
2. **Unwired production surface** (port methods/structs no caller ever reaches)
   → delete it.
3. **Referenced only from tests** → first try to rewrite the test so the item is
   not needed; only when that would duplicate production logic or delete
   meaningful coverage is the item kept with `#[cfg(test)]` (the convention the
   codebase already used for `historical_urls` / `wfs_page_size` /
   `attached_state`).
4. No new `#[allow(dead_code)]` anywhere. The one pre-existing per-item allow
   (`muenster_github/parsing.rs`, `start` field) was removed with its field.

## Deleted

### Driven adapters

- `cache_index` test accessors (bonn, leipzig) — unused even by tests.
- `HttpResourceFetcher::with_bearer`, `UpstreamHeaders::is_empty`,
  `SourceScanner::reset`.
- `RawAttributes.address_country` / `.address_region` (scraping),
  `RawSiteMetadata.id_pdc` (+ its assertion), `RawSite.domain_id` / `.interval`,
  `RawDataPoint.iso_date`, `RawFeatureCollection.number_returned`.
- `EcoCounterV1Provider::TIMEZONE`, hamburg `IOT_NEXT_LINK` / `RawFeature` /
  `RawGeometry.geometry_type`, `EcoCounterV2Adapter::step` field + getter (+ the
  assertion).
- `ArchiveIndex.extracted_dir`, postgres `EXPORT_TIMEZONE`.
- `normalize_column_name` — the tests now call its production caller
  `header_parts` (`pub(super)`), so the wrapper is gone.
- REST test helpers `DATA_SOURCE_ID_B` and `MockOpenDataFileRepository::seed`.

### Multi-resolution seam (never wired into production)

- `ResolutionCoverage`, `ChannelCoverage`,
  `MeasurementRepository::resolution_coverage` /
  `resolution_coverage_by_channel` (+ postgres impl and its DB test),
  `station_analytics::resolution::{select_resolution, covers_window, covers}`
  and their unit tests. [`introduced_after`](backend/src/core/application/station_analytics/resolution.rs)
  stays — it is used by the analytics.

### Core application

- `DataSourceRuntime.configuration` and `DataImportService::runtimes` (incl. the
  constructor parameter — 22 call sites updated).
- `JobHeartbeat.cancelled` / `cancelled_flag` (no caller consumed the flag; the
  loop still exits when the job is no longer RUNNING). The heartbeat test now
  waits for the third beat instead of polling the flag.
- `MemoryJobRepository.locks`, the second unused `DataSourceRuntime` test
  constructor helper, `MeasurementRollupService::refresh` + its delegation test.
- `DataSourceUpdateServicePort` (trait, impl, test usage).

### Core domain

- Duplicate `ServiceHealthIndicator` trait in `health/indicator.rs` (the real
  port is `health/indicator_port.rs`, used by `HealthService`).
- `DataSource.logo_sha256`, `DataSourceRepository::update_logo`,
  `DataProvider::get_data_source_image` + `DataSourceImage`,
  `PersistentStateAccess::clear` (+ impls and its scoping test).
- `DataSourceRepository::find_by_name` (+ every impl),
  `MeasurementRepository::find_all` (+ every impl), `MeasurementRepository::save`
  is retained only for DB tests (see below).
- Unused config accessors: the four `*_max_heartbeat_interval_seconds` getters
  and `MapsConfiguration::update_max_heartbeat_interval_seconds`. The tests now
  assert the used `*_max_heartbeat_interval().num_seconds()` path instead.
- `DataProviderConfiguration::vars` + its test, `ObjectKey::parse` + its test,
  `GeoBounds::contains` + its test, `Job::{is_running, is_successful,
  is_cancellation_requested, is_cancelled, is_terminal}`,
  `MetricKey::ALL`, `ImportRun::failed`.

## Tests rewritten (instead of keeping the helper)

- `Job` predicate assertions → `assert_eq!(job.status, JobStatus::…)` in
  `job.rs`, `job_service.rs` and `job_reconciliation_service.rs`.
- `ImportRun::failed()` → `assert_eq!(run.status, ImportRunStatus::…)` (and the
  failure-message test now asserts the message).
- `MetricKey::ALL` → an explicit four-element array in the test.
- `utc` fixture helpers: hamburg and eco-counter v2 parsers own them inside their
  `mod tests`; hamburg's is shared by two test modules, so it stays a
  `#[cfg(test)]` fixture next to the parser, like `eco_counter::common::utc`.

## Remaining `#[cfg(test)]` (deliberate)

- **Config accessors** (`cache_duration_secs` ×5, hamburg `concurrency`,
  muenster's three) + the pre-existing `historical_urls` / `wfs_page_size` /
  `attached_state`. The parsed value has no other observable effect, so the
  accessor *is* the only way to keep the config-parsing assertions.
- **Shared test fixtures** `eco_counter::common::utc` and
  `hamburg_sta::parsing::utc`.
- **`MeasurementRepository::{save, find_by_channel_id, sum_buckets, sum_weekdays}`
  (+ `sum_weekdays_raw`) and the postgres overrides.** These read/write paths are
  exercised only by Postgres DB tests; rewriting those tests would mean
  re-deriving the queries with `save_batch` / `sum_buckets_by_channel`, i.e.
  duplicating the production SQL shape in test code.
- **`DataImportService::update_data_source`** — a no-op-phase convenience
  wrapper used by ~20 unit tests; production calls
  `update_data_source_with_progress`.

## Definition of done

- [x] `#![allow(dead_code)]` removed; no `allow(dead_code)`/`expect(dead_code)`
      left in `backend/src` or `frontend/src`
- [x] `make check` green (`cargo fmt --check`, `cargo clippy --all-targets -- -D
      warnings`, prettier, `cargo audit`)
- [x] `make test` green (836 tests)
- [x] `make test-rest` green (130 tests)
- [x] `make coverage` green — overall production line coverage 87.26 % (≥ 80 %),
      core 97.28 % (≥ 95 %)
- [x] Docs/tests updated: no dangling references to removed items
