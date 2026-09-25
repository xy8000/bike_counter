# 145 - Coverage top-up for the four files with the most missed lines

Status: implemented

## Result

Backend gate: **overall (production) 87.03 %** (≥ 80 %) and **core 97.16 %**
(≥ 95 %) — green, 809 tests.

| File | Before | After |
|---|---|---|
| `data_source_update_service.rs` | 84.06 % | **86.14 %** (production-only 99.32 %) |
| `muenster_github/adapter.rs` | 85.52 % | **96.21 %** |
| `main.rs` | 19.78 % | **25.75 %** |
| `tiles_update_service.rs` | 87.09 % | **95.46 %** |
| `job_scheduler.rs` (bonus, see plan 146) | 0.00 % | **61.76 %** |

## Context

A code-coverage report flagged four backend files as having the most missed
lines (and, for three of them, a coverage decrease):

| File | Missed lines (report) | Line coverage | Branch coverage | Change |
|---|---|---|---|---|
| `backend/src/core/application/data_source_update_service.rs` | 31 | 84.06 % | 66.67 % | −1.02 % |
| `backend/src/adapter/driven/muenster_github/adapter.rs` | 15 | 85.52 % | 61.54 % | −1.74 % |
| `backend/src/main.rs` | 15 | 19.78 % | 0.00 % | −0.66 % |
| `backend/src/core/application/tiles_update_service.rs` | 11 | 87.09 % | 52.94 % | +0.68 % |

The production-only coverage gate requires overall ≥ 80 % and `src/core/` ≥
95 %. The two core services sit below the core bar, so the missed lines are a
real risk to the gate. This plan adds focused unit tests for the genuinely
reachable production branches in those four files — no threshold is lowered and
no production behaviour is changed.

## Approach

1. Refresh the LLVM coverage report and extract the exact uncovered
   **production** lines per file (lines before the first `#[cfg(test)]` marker).
2. For each file, add unit tests that drive the missing branches through the
   existing in-memory doubles, only adding fault-injection knobs (e.g. a failing
   heartbeat) where a branch is currently unreachable.
3. Re-run the coverage gate (`make coverage`) plus `make check` / `make
   test-rest` and confirm the four files' production line coverage rises.

## Target branches

### `data_source_update_service.rs`
- `ScheduledJobPort::run_if_due` and `DataSourceUpdateServicePort::run_if_due`
  trait-wrapper methods (never called through the trait in tests).
- `check_cancellation` heartbeat-error path (needs a job repo whose `heartbeat`
  can be made to fail).
- `run_updates` thread-panic branch (a provider that panics in its worker).
- `is_overdue` fallback for a FINISHED job without timestamps.
- The error branches of the best-effort bookkeeping (`mark_cancelled`,
  record-FINISHED metadata, finish/fail import-run) where still uncovered.

### `muenster_github/adapter.rs`
- Uncovered cache/config error paths, `Tier 4` upstream-unchanged reuse, and the
  empty-channel / no-data paging branches in `page_channel` / `windowed_series`
  / `earliest_timestamp`.

### `main.rs`
- `builtin_images` / `content_type_for` helper branches and the tiles
  subcommand/config wiring that can be exercised without a database.

### `tiles_update_service.rs`
- `ScheduledJobPort::run_if_due` wrapper, the `is_overdue` fallback, the
  pre-build cancellation path, the failure branch, and the provisioning error
  mapping via `run_update`.

## Definition of done

- [x] Plan file in `plans/` created and kept current
- [x] New tests added for all four target files
- [x] `make check` green
- [x] `make test-rest` (and/or `make test`) green (809 tests)
- [x] `make coverage` green (backend production lines overall ≥ 80 %, core ≥ 95 %)
- [x] Coverage of the four target files measurably increased
- [x] README / plan docs updated as needed (no README-impacting behaviour change)
