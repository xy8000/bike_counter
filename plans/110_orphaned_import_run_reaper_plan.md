# 110 - Finalize orphaned per-source import runs (Eco-Counter "Running" despite cancelled jobs)

Status: implemented

## Problem

The data-sources page shows `Eco-Counter` with `Last import: Running · 21m 50s`
while the Jobs page shows every job as `CANCELLED`. The user restarted jobs, so
the database no longer reflects the exact state, but the underlying defect is
still present.

### Root cause

There are **two independent bookkeeping tables**, and only one of them has a
reconciliation/cancel path:

| Table | Row | Drives | Finalized by |
|---|---|---|---|
| `jobs` | aggregate `data_source_update` job | the Jobs page | worker `finalize`, REST `cancel` (incl. `force`), `JobReconciliationService` |
| `data_source_imports` | per-source [`DataImportRun`](backend/src/core/domain/data_source/import_run.rs:54) | the data-sources "Last import" badge | **only** the owning worker thread |

The "Last import → Running" badge is read from `data_source_imports`, **not**
from `jobs`:

- [`DataSourceAnalyticsService::overview`](backend/src/core/application/data_source_analytics_service.rs:70)
  and [`detail`](backend/src/core/application/data_source_analytics_service.rs:126) call
  [`latest_by_data_source`](backend/src/adapter/driven/postgres/import_run_repository.rs:107).
- [`ImportStatus`](frontend/src/features/dataSources/ImportStatus.tsx:31) renders
  `Running · <elapsed>` whenever that row has `status = 'RUNNING'` (the elapsed
  time is computed client-side from `started_at`).

The per-source run is only ever transitioned by the worker that started it, in
[`DataSourceUpdateService::update_one_source`](backend/src/core/application/data_source_update_service.rs:343):

- success → [`finish`](backend/src/core/application/data_source_update_service.rs:432)
- [`DomainError::Cancelled`](backend/src/core/application/data_source_update_service.rs:437) → `finish`
- error → [`fail`](backend/src/core/application/data_source_update_service.rs:458)

Cancellation is only noticed **at a batch boundary**, via
[`check_cancellation`](backend/src/core/application/data_source_update_service.rs:480)
inside the `on_batch` callback. The aggregate `data_source_update` job, however,
can be terminated **without the worker ever returning**:

- REST [`cancel_job`](backend/src/adapter/driving/rest/handlers/jobs.rs:82) with
  `{"force": true}` → [`mark_cancelled`](backend/src/adapter/driven/postgres/job_repository.rs:259)
  flips the `jobs` row to `CANCELLED` immediately.
- [`JobReconciliationService::reconcile_all`](backend/src/core/application/job_reconciliation_service.rs:57)
  → [`reconcile_stale_active`](backend/src/adapter/driven/postgres/job_repository.rs:399)
  force-cancels a stale-heartbeat job (`CANCELLATION_REQUESTED` → `CANCELLED`).

Neither path touches `data_source_imports`. If the worker is stuck inside a
single provider call it never reaches `check_cancellation` or the `finish`/`fail`
call, so the per-source row stays `RUNNING` forever while the aggregate job is
already `CANCELLED`.

The Eco-Counter V1 source is especially prone to this:

- [`get_measurements_source`](backend/src/adapter/driven/eco_counter/v1/adapter.rs:500)
  first calls [`ensure_index`](backend/src/adapter/driven/eco_counter/v1/adapter.rs:217),
  which fetches metadata for **every** catalog station before the first batch is
  ever returned.
- Each fetch goes through [`HttpResourceFetcher`](backend/src/adapter/driven/eco_counter/fetcher.rs:54),
  which calls `ureq::get(...).call()` with **no explicit read/connect timeout**,
  and [`client.data`](backend/src/adapter/driven/eco_counter/v1/client.rs:77) pages
  the cumulative series over day windows. A slow/hung upstream can therefore
  block the worker thread for an unbounded time.

This failure mode is **not Eco-Counter-specific**. Every other HTTP adapter
fetcher uses the same unbounded `ureq::get(...).call()` pattern with no explicit
read/connect timeout:

- [`muenster_github/fetcher.rs`](backend/src/adapter/driven/muenster_github/fetcher.rs:53)
- [`bonn_opendata/fetcher.rs`](backend/src/adapter/driven/bonn_opendata/fetcher.rs:14)
- [`hamburg_sta/fetcher.rs`](backend/src/adapter/driven/hamburg_sta/fetcher.rs:37)
- [`leipzig_wfs/fetcher.rs`](backend/src/adapter/driven/leipzig_wfs/fetcher.rs:14)

So any source can leave an orphaned `RUNNING` import run if its worker is stuck
in a hung HTTP call (or the backend restarts mid-import). The reaper (Tasks 1–3)
is therefore data-source-agnostic and covers every configured source; the
timeout hardening (Task 5) is likewise applied to all fetchers.

## Goal

Guarantee that a `data_source_imports` row can never remain `RUNNING` after its
aggregate `data_source_update` job has reached a terminal state, without
requiring the owning worker thread to be alive or responsive. This resolves both
the force-cancel case reported here and the backend-restart case diagnosed in
[`105_hessen_import_runs_forever_plan.md`](plans/105_hessen_import_runs_forever_plan.md:19).

## Approach

### Task 1 — Port + domain

Add a reaper method to
[`DataImportRunRepository`](backend/src/core/domain/data_source/import_run_port.rs:14):

```rust
/// Finalizes RUNNING import runs that can no longer be owned by a live worker:
/// a run whose aggregate job is terminal, or an unlinked run (job_id IS NULL)
/// older than `older_than`. Returns the number of rows finalized.
fn finalize_orphaned_running(&self, older_than: DateTime<Utc>) -> Result<u64, DomainError>;
```

Update every in-memory test double of the trait (e.g. the mocks in
[`data_source_analytics_service.rs`](backend/src/core/application/data_source_analytics_service.rs:594)
and [`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:1369),
plus any REST test doubles) with a default no-op/recording implementation.

### Task 2 — Postgres implementation

Implement [`finalize_orphaned_running`](backend/src/adapter/driven/postgres/import_run_repository.rs:42)
as two atomic `UPDATE`s:

1. Job-linked orphans — a `RUNNING` import run whose `job_id` points at a job in a
   terminal state (`FINISHED`/`FAILED`/`CANCELLED`) is finalized as `FINISHED`
   with `finished_at = COALESCE(j.finished_at, now())` (so the UI duration is
   accurate):

   ```sql
   UPDATE data_source_imports AS dsi
   SET status = 'FINISHED',
       finished_at = COALESCE(j.finished_at, now()),
       failure_message = NULL
   FROM jobs j
   WHERE dsi.status = 'RUNNING'
     AND j.id = dsi.job_id
     AND j.status IN ('FINISHED', 'FAILED', 'CANCELLED');
   ```

   This is safe: a `FINISHED` aggregate job means every worker already returned
   and finalized its own run (the only way a run is still `RUNNING` under a
   terminal job is an orphan — a crash, a force-cancel, or a non-fatal
   `finish`-write failure).

2. Unlinked-orphan safety net — `job_id IS NULL` and `started_at < older_than`:

   ```sql
   UPDATE data_source_imports
   SET status = 'FINISHED', finished_at = now(), failure_message = NULL
   WHERE status = 'RUNNING'
     AND job_id IS NULL
     AND started_at < $1;
   ```

   (In practice every run is created with a `job_id`; this covers the model's
   documented `None` case and legacy rows.)

`FINISHED` (not `FAILED`) is chosen to mirror the existing cooperative-cancel
behavior and to avoid a spurious "last import failed" banner for a run that was
merely interrupted. Add repository tests (job-terminal orphan, unlinked-orphan,
and a still-RUNNING job's run is spared).

### Task 3 — Reconcile on the existing watcher tick

Extend
[`JobReconciliationService`](backend/src/core/application/job_reconciliation_service.rs:19)
to also hold `Arc<dyn DataImportRunRepository>` and call
`finalize_orphaned_running(now - grace)` at the end of
[`reconcile_all`](backend/src/core/application/job_reconciliation_service.rs:57).
Use a fixed grace constant (e.g. 15 minutes) for the unlinked-orphan safety net;
the job-terminal case needs no grace because it is deterministic.

This reuses the existing 30-second
[`run_job_watcher`](backend/src/adapter/driving/job_scheduler.rs:53) loop, so the
UI self-heals within one watcher tick after a job is cancelled. Update the
service's unit tests to cover both orphan cases.

### Task 4 — Wiring

In [`main.rs`](backend/src/main.rs:287):

- Clone `import_run_repo` **before** it is moved into
  [`DataSourceAnalyticsService`](backend/src/main.rs:257).
- Pass the clone into
  [`JobReconciliationService::new`](backend/src/core/application/job_reconciliation_service.rs:25).

### Task 5 — Hardening (optional but recommended)

Give every HTTP adapter fetcher an explicit timeout so a hung upstream cannot
block a worker thread for an unbounded time. They all currently call
`ureq::get(...).call()` with no read/connect timeout:

- Eco-Counter [`HttpResourceFetcher`](backend/src/adapter/driven/eco_counter/fetcher.rs:17)
  (V1 + V2) and scraping [`HttpPageFetcher`](backend/src/adapter/driven/eco_counter/scraping/fetcher.rs:27)
  (web provider).
- [`muenster_github/fetcher.rs`](backend/src/adapter/driven/muenster_github/fetcher.rs:53)
  (Münster GitHub ZIP download).
- [`bonn_opendata/fetcher.rs`](backend/src/adapter/driven/bonn_opendata/fetcher.rs:14)
  (Bonn GeoJSON/CSV).
- [`hamburg_sta/fetcher.rs`](backend/src/adapter/driven/hamburg_sta/fetcher.rs:37)
  (Hamburg SensorThings API).
- [`leipzig_wfs/fetcher.rs`](backend/src/adapter/driven/leipzig_wfs/fetcher.rs:14)
  (Leipzig WFS).

Add an optional per-provider var `request_timeout_seconds` (default 30 s), parsed
through the shared helper [`driven/http.rs`](backend/src/adapter/driven/http.rs:1)
(`parse_request_timeout` + `timed_agent`), which builds a ureq agent with an
end-to-end `timeout_global` (ureq 3.4's API — DNS → connect → whole response
body) so a hung upstream can never block a worker thread. This is complementary:
it reduces how long a worker can be stuck, while the reaper (Tasks 1–3)
guarantees the stale `RUNNING` badge is cleared even if a call still hangs.

### Task 6 — Tests, gates, docs

- Repository tests (Task 2) and reconciliation-service tests (Task 3).
- Run `make check`, `make test`, `make test-rest`, `make coverage`
  (and `make test-playwright` only if the frontend changes).
- Update [`README.md`](README.md) job-lifecycle notes and register this plan in
  [`plans/README.md`](plans/README.md:1).

## Files touched

| File | Change |
|---|---|
| [`backend/src/core/domain/data_source/import_run_port.rs`](backend/src/core/domain/data_source/import_run_port.rs:14) | `finalize_orphaned_running` port method |
| [`backend/src/adapter/driven/postgres/import_run_repository.rs`](backend/src/adapter/driven/postgres/import_run_repository.rs:42) | SQL reaper + tests |
| [`backend/src/core/application/job_reconciliation_service.rs`](backend/src/core/application/job_reconciliation_service.rs:19) | hold + call the import-run reaper |
| [`backend/src/main.rs`](backend/src/main.rs:287) | clone + pass `import_run_repo` |
| [`backend/src/core/application/data_source_analytics_service.rs`](backend/src/core/application/data_source_analytics_service.rs:594) | test-double update |
| [`backend/src/core/application/data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:1369) | test-double update |
| [`backend/src/adapter/driven/eco_counter/fetcher.rs`](backend/src/adapter/driven/eco_counter/fetcher.rs:17) | optional request timeout, V1/V2 (Task 5) |
| [`backend/src/adapter/driven/eco_counter/scraping/fetcher.rs`](backend/src/adapter/driven/eco_counter/scraping/fetcher.rs:27) | optional request timeout, web scraper (Task 5) |
| [`backend/src/adapter/driven/muenster_github/fetcher.rs`](backend/src/adapter/driven/muenster_github/fetcher.rs:53) | optional request timeout (Task 5) |
| [`backend/src/adapter/driven/bonn_opendata/fetcher.rs`](backend/src/adapter/driven/bonn_opendata/fetcher.rs:14) | optional request timeout (Task 5) |
| [`backend/src/adapter/driven/hamburg_sta/fetcher.rs`](backend/src/adapter/driven/hamburg_sta/fetcher.rs:37) | optional request timeout (Task 5) |
| [`backend/src/adapter/driven/leipzig_wfs/fetcher.rs`](backend/src/adapter/driven/leipzig_wfs/fetcher.rs:14) | optional request timeout (Task 5) |
| [`backend/src/adapter/driven/http.rs`](backend/src/adapter/driven/http.rs:1) | new shared ureq timeout agent + `request_timeout_seconds` parser (Task 5) |
| [`backend/src/adapter/driven/mod.rs`](backend/src/adapter/driven/mod.rs:1) | register `http` module |
| the six `*/adapter.rs` `new()` builders | parse `request_timeout_seconds` and build their fetcher with `with_timeout` (Task 5) |
| [`README.md`](README.md) | lifecycle notes |
| [`plans/README.md`](plans/README.md:1) | registration |

## Definition of done

- [x] Plan registered in [`plans/README.md`](plans/README.md:1)
- [x] `finalize_orphaned_running` added to the port, Postgres impl and all test doubles
- [x] A `RUNNING` import run under a terminal job is finalized within one watcher tick
- [x] Unlinked `RUNNING` runs older than the grace window are finalized
- [x] A still-`RUNNING` job's import run is never touched
- [x] Every HTTP adapter fetcher runs on a ureq agent with an end-to-end timeout
      (optional `request_timeout_seconds`, default 30 s)
- [x] `make check` / `make test` / `make test-rest` / `make coverage` green
- [x] README / plan docs updated
