# 89 - Adapter-owned incremental import (Hamburg fix) plan — 3 pieces

Status: All three pieces complete — `make check` (fmt/clippy/audit/prettier),
`make test` (547 passed) and `make coverage` (overall 86.4%, core 95.1%) green.
Piece 3 removed the per-channel `get_measurements`/`MeasurementQuery`/
`MeasurementBatch`/`supports_source_read` and the one-shot `import()`/
`import_measurements()`/`ImportSummary`: `get_measurements_source` is now the
single required measurement method on `DataProvider`,
`DataImportService::update_data_source` is the single source-level loop
(deadline + per-batch watermark checkpointing) and `run_updates` no longer
branches on `supports_source_read`. The five test `DataProvider` mocks and the
three adapter test suites were migrated to the source-level read (which provides
the adapter source-level coverage); per-channel-only tests were removed and
coverage-raising error-path tests added.

## Problem

The Hamburg data-source update job never completes and keeps restarting a full
re-import. The job history shows a repeating cycle: a `data_source_update` job
starts, imports a few million measurements, runs past its `lifetime_until`, the
backend restarts, and on startup
[`DataSourceUpdateService::run_if_due`](backend/src/core/application/data_source_update_service.rs:76)
expires the stale RUNNING job as FAILED "Max lifetime exceeded" and starts over.

Root cause:

- [`DataSourceUpdateService::run_updates`](backend/src/core/application/data_source_update_service.rs:222)
  only persists `imported_until` **after** a source's full update succeeds.
- [`DataImportService::update_data_source`](backend/src/core/application/data_import_service.rs:402)
  drains each channel **completely** before the next, so the shared
  `imported_until` cannot advance mid-run.

The Hamburg backfill (years of 5-min data across many fields) exceeds the
lifetime, so the watermark never advances and every run reprocesses everything.

## Goals

- Persist the import watermark **incrementally** so an interrupted run resumes.
- Stop gracefully at `lifetime_until` (FINISHED, not FAILED).
- Keep the core agnostic about **how** a provider reads its measurements: the
  interleaving/lockstep and the per-channel cursor bookkeeping live in the
  adapter. The core only consumes a source-level batch and a safe watermark.
- Keep the core-owned `imported_until` watermark (domain, repository port, REST
  DTO, reset endpoint) — the authoritative, resettable cursor.

## Delivery: three independent pieces

Each piece lands with `make check` + `make test` green on its own.

### Piece 1 — source-level read capability + Hamburg (fixes the bug)

Add the source-level read to the provider port **without breaking anything**:

- [`provider_port.rs`](backend/src/core/domain/data_source/provider_port.rs):
  add `SourceMeasurement` / `SourceMeasurementBatch` and two **defaulted**
  trait methods — `supports_source_read(&self) -> bool { false }` and
  `get_measurements_source(&self, from, max_batch_size) -> Result<SourceMeasurementBatch, ProviderError>`
  returning an empty `more: false` batch by default. Existing providers and
  mocks keep compiling unchanged.
- Add the driven [`SourceScanner`](backend/src/adapter/driven/source_merge.rs)
  helper (fair channel interleave + safe min-real watermark) and register it in
  [`driven/mod.rs`](backend/src/adapter/driven/mod.rs).
- [`HamburgStaAdapter`](backend/src/adapter/driven/hamburg_sta/adapter.rs):
  override `supports_source_read() -> true` and `get_measurements_source` using
  the scanner plus a `page_channel` that pages one field (legacy+current merge,
  same dedup rules) and reports a real `next_from`; keep `get_measurements`.
- [`DataImportService`](backend/src/core/application/data_import_service.rs):
  add a new `update_data_source_source(runtime, from, deadline, on_batch(processed, added, watermark))`
  loop over `get_measurements_source` (group by channel, save, checkpoint) that
  stops at the deadline with `completed: false`. The existing per-channel
  `update_data_source` / `import` stay untouched.
- [`DataSourceUpdateService::run_updates`](backend/src/core/application/data_source_update_service.rs:222):
  branch on `supports_source_read()` — source path passes `lifetime_until`,
  checkpoints `imported_until` per batch and finishes gracefully on a deadline
  stop; per-channel providers keep the old path.

Tests: Hamburg source-level read (interleave + watermark), the source-loop
deadline/checkpoint, plus the existing suite (untouched).

### Piece 2 — migrate Münster and Bonn to the source-level read

- [`MuensterGithubAdapter`](backend/src/adapter/driven/muenster_github/adapter.rs):
  implement `get_measurements_source` via the scanner (per-channel timeframe
  windows + gap-skip preserved) and `supports_source_read() -> true`.
- [`BonnOpendataAdapter`](backend/src/adapter/driven/bonn_opendata/adapter.rs):
  implement `get_measurements_source` via the scanner (in-memory rows, row-count
  paging) and `supports_source_read() -> true`.
- Add adapter-level tests for each source-level read; keep the existing
  per-channel tests until Piece 3.

After this piece every real provider serves the source-level read; the core's
per-channel fallback remains only for tests/legacy `import()`.

### Piece 3 — remove the per-channel API and the one-shot `import()`

- [`provider_port.rs`](backend/src/core/domain/data_source/provider_port.rs):
  remove `get_measurements`, `MeasurementQuery`, `MeasurementBatch` and
  `supports_source_read`; make `get_measurements_source` the single required
  measurement method.
- [`data_import_service.rs`](backend/src/core/application/data_import_service.rs):
  remove the per-channel fallback, `import()`, `import_measurements()` and
  `ImportSummary`; `update_data_source` becomes the single source-level loop.
- Update the five test `DataProvider` mocks and the three adapter test suites to
  the source-level method; remove the `import()`-only tests.
- `run_updates` drops the per-channel branch.

## Definition of done (per piece)

- [x] `make check` green (final: fmt/clippy/audit/frontend prettier OK)
- [x] `make test` green (final: 547 passed)
- [x] `make coverage` green (final: overall 86.4%, core 95.1%)
