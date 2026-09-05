# 104 — Run all data-source imports in parallel (one shared job) with per-source metadata

## Context / problem

The `data_source_update` job currently imports every configured data source
**sequentially**. [`DataSourceUpdateService::run_updates`](backend/src/core/application/data_source_update_service.rs:226)
loops over `self.runtimes` one by one, and each source's full import
(`DataImportService::update_data_source`) finishes before the next starts. With
six production data sources the job wall-clock time is the **sum** of all source
durations, even though each source is I/O-bound (blocking `ureq` HTTP + Postgres
round-trips) and could overlap.

Progress is tracked with two **flat** job-metadata keys shared by every source —
`processed_measurements` and `added_measurements` (see the constants in
[`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:33)) —
so the counts of different sources overwrite each other and there is no
per-source import status visible on the job.

## Goals

1. Start all data sources in parallel while keeping **one** aggregate
   `data_source_update` job (the ShedLock-style `jobs` row stays single).
2. Keep the existing synchronous `DataProvider` / blocking `ureq` / blocking
   Postgres stack — concurrency is achieved with one blocking thread per data
   source (Option 2), not an async trait migration.
3. Group the import counters per data source under the data source UUID:
   `{data_source_id}_processed_measurements`, `{data_source_id}_added_measurements`.
4. Add a per-source `{data_source_id}_status` field reporting the import phase it
   is currently on: `STARTING`, `SYNCING_STATIONS`, `SYNCING_CHANNELS`,
   `IMPORTING_MEASUREMENTS`, then `FINISHED` or `FAILED`.
5. Preserve per-source semantics: watermark checkpointing into `imported_until`,
   measurement-bound updates, `last_updated_at` stamping, and per-source
   `DataImportRun` bookkeeping (start/finish/fail).

## Changes

### 1. Import phase model + progress callback ([`data_import_service.rs`](backend/src/core/application/data_import_service.rs:39))

- Add a public phase enum next to `DataSourceUpdate`:

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum DataSourceImportPhase {
      Starting,
      SyncingStations,
      SyncingChannels,
      ImportingMeasurements,
      Finished,
      Failed,
  }
  ```

  with `as_str()` returning `STARTING`, `SYNCING_STATIONS`, `SYNCING_CHANNELS`,
  `IMPORTING_MEASUREMENTS`, `FINISHED`, `FAILED`.
- Add [`DataImportService::update_data_source_with_progress`](backend/src/core/application/data_import_service.rs:361)
  with a phase callback `on_phase: impl FnMut(DataSourceImportPhase) -> Result<(), DomainError>`
  so the caller can persist the current phase. The existing
  [`update_data_source`](backend/src/core/application/data_import_service.rs:359)
  delegates to it with a no-op phase reporter, so existing callers and test
  doubles stay unchanged. Invoke `on_phase`:
  - `Starting` at method entry,
  - `SyncingStations` around [`sync_counting_stations`](backend/src/core/application/data_import_service.rs:104),
  - `SyncingChannels` around [`sync_channels`](backend/src/core/application/data_import_service.rs:278),
  - `ImportingMeasurements` before the measurement paging loop.
- `Finished` / `Failed` are **not** emitted here; the caller sets them from the
  returned `Result` (see change 2) so the final status is authoritative.
- Add a unit test asserting the phase
  sequence `Starting → SyncingStations → SyncingChannels → ImportingMeasurements`
  via the progress method.

### 2. Per-source metadata keys ([`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:27))

- Replace the flat key constants `PROCESSED_MEASUREMENTS_KEY` /
  `ADDED_MEASUREMENTS_KEY` with builder functions keyed by the data source UUID
  ([`DataSource::id_from_name`](backend/src/core/domain/data_source/data_source.rs:59)
  produces the deterministic `DataSourceId`):

  ```rust
  pub fn processed_measurements_key(data_source_id: DataSourceId) -> String
  pub fn added_measurements_key(data_source_id: DataSourceId) -> String
  pub fn status_key(data_source_id: DataSourceId) -> String
  ```

  each formatting `"{data_source_id.0}_{suffix}"` (the UUID's standard hyphenated
  `Display`).
- The `on_batch` closure writes the two per-source counter keys, and the new
  `on_phase` closure writes `status_key(...)` with the phase string.
- Keep `JobRepository::update_metadata` unchanged: its
  [`jsonb_set`](backend/src/adapter/driven/postgres/job_repository.rs:147) UPDATE
  is atomic per key, so concurrent writers on different keys are safe.

### 3. Parallel orchestration ([`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:226))

- Extract the current per-source loop body into a private helper
  `update_one_source(&self, job_id, deadline, runtime) -> Result<(), DomainError>`
  that:
  1. reads `imported_until` for the runtime,
  2. inserts a `DataImportRun::start(...)`,
  3. calls `update_data_source` with the per-source `on_batch` (counters +
     watermark checkpoint) and `on_phase` (status writes),
  4. on `Ok`: stamps `last_updated_at` when `update.completed`, persists
     measurement bounds, writes `status_key = FINISHED`, finishes the import run;
  5. on `Err`: writes `status_key = FAILED`, fails the import run, returns the error.
- Rewrite `run_updates` to spawn one OS thread per runtime via
  `std::thread::scope` (safe because the method is already invoked from a
  blocking context — the scheduler wraps `run_if_due` in
  [`spawn_blocking`](backend/src/adapter/driving/job_scheduler.rs:22), and
  `std::thread::scope` threads are not tokio workers). Each scoped thread clones
  the `Arc` repositories and calls `update_one_source` for its own runtime.
- Collect every thread's result and return:
  - `Ok(())` when all sources succeed,
  - `Err(DomainError::Provider(...))` aggregating the failing source ids/messages
    when any source fails — so all sources still run to completion while the
    aggregate job is marked FAILED (preserving the existing all-or-nothing job
    status semantics, now without short-circuiting).

### 4. Tests ([`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:325))

- Update assertions that read the flat `processed_measurements` /
  `added_measurements` keys to use `processed_measurements_key(...)` /
  `added_measurements_key(...)` with `DataSource::id_from_name("Münster")`.
- Add a test that two runtimes actually run concurrently (e.g. each provider
  blocks on a shared barrier before returning a batch, asserting both threads
  reach the barrier).
- Add a test that each source writes its own `{uuid}_added_measurements`,
  `{uuid}_processed_measurements` and ends with `{uuid}_status = FINISHED`, and
  that a failing source ends with `{uuid}_status = FAILED` while the job is
  still FAILED.
- The existing mock repositories already use `Mutex`, so they are safe under the
  new concurrency; no port changes are required.

### 5. Docs

- [`backend/src/adapter/driving/rest/dto/jobs.rs`](backend/src/adapter/driving/rest/dto/jobs.rs:41)
  — update the `metadata` doc-comment example from `processed_measurements` to
  the per-source key shape (e.g. `<data-source-uuid>_processed_measurements`).
- [`plans/README.md`](plans/README.md:11) — register this plan as the current
  plan.
- No frontend change: the data-sources pages already read per-source
  `DataImportRun` status via the BFF, not the job metadata.

## Execution order

1. Add `DataSourceImportPhase` + `on_phase` to `DataImportService` and update
   its tests; run `make test-rest`.
2. Add the per-source metadata key helpers and rewrite `run_updates` to parallel
   scoped threads + `update_one_source`; wire the phase/status and counter
   metadata writes.
3. Update/add `DataSourceUpdateService` tests; run `make test-rest`.
4. Update the jobs DTO doc comment and register the plan.
5. Gates: `make check`, `make test-rest`, `make coverage` (all green; 625 tests,
   core 95.13%). `make test-playwright` is **not** required: this is a
   backend-only change that does not touch the frontend UI, and the e2e run
   disables the schedulers (`scheduled_jobs_enabled = false`), so it would not
   exercise the new parallel import path.

## Definition of done

- [x] All data sources run in parallel under one `data_source_update` job;
      `make test` / `make test-rest` green.
- [x] Job metadata uses `{data_source_id}_processed_measurements`,
      `{data_source_id}_added_measurements`, `{data_source_id}_status`
      (UUID-keyed); flat keys removed.
- [x] `{data_source_id}_status` transitions
      STARTING → SYNCING_STATIONS → SYNCING_CHANNELS → IMPORTING_MEASUREMENTS →
      FINISHED / FAILED.
- [x] A failing source still fails the aggregate job, while other sources finish.
- [x] `make check` green.
- [x] `make coverage` green (overall ≥ 80%, core ≥ 95%).
- [x] `make test-playwright` not required (backend-only change, no UI touched).
- [x] Docs (`jobs` DTO comment, plan registry) updated.
