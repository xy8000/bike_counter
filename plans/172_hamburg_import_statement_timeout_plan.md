# 172 - Hamburg import cancelled by the new statement timeout (investigation)

Status: implemented — maintenance transactions lift the request
`statement_timeout` and import batches are written in bounded chunks
([`measurement_repository.rs`](backend/src/adapter/driven/postgres/measurement_repository.rs:350)).
Evidence was gathered read-only from the live instance
`https://bikecounter.lpeters3.ddnss.de` (backend `0.0.10`) and from a local
PostgreSQL reproduction of the production `measurements` schema; the fix is
covered by the `save_batch` repository test.

## Symptom

The Hamburg data source (`48c6f5c7-8c8d-559f-b568-ad7e7e52e4d3`) reports

```
The last import failed: Database("canceling statement due to statement timeout (SQLSTATE 57014)")
```

and its data has been frozen since `2026-11-01T03:00:00Z`.

## What the live instance shows

### 1. The failure is new, daily and identical

Hamburg import runs (`GET /api/bff/data-sources/{id}/imports`):

| started | duration | result |
|---|---|---|
| 2026-11-07T03:00:00Z | 24 s | FAILED `Database("canceling statement due to statement timeout (SQLSTATE 57014)")` |
| 2026-11-06T03:00:00Z | 18 s | same |
| 2026-11-05T03:00:00Z | 23 s | same |
| 2026-11-04T03:00:00Z | 18 s | same |
| 2026-11-03T03:00:00Z | 21 s | same |
| 2026-11-02T03:00:00Z | 22 s | same |
| **2026-11-01T03:00:00Z** | **810 s** | **FINISHED (35 777 rows)** |
| 2026-10-31T16:49:46Z | 667 s | FINISHED (47 761 rows) |
| 2026-10-30T03:00:00Z | 900 s | FINISHED (100 193 rows) |

`last_updated_at = 2026-11-01T03:13:31Z`, `first_data_at = 2026-06-27`,
`last_data_at = 2026-11-01T03:00:00Z`, `imported_until = 2026-11-01T03:00:00Z`.
So **no watermark checkpoint has been written since Nov 1** — every daily run
re-attempts exactly the same work and dies in the same place.

### 2. The run dies before its first batch is checkpointed

The aggregate `data_source_update` job metadata of every failing run contains
**only** `48c6…_status = FAILED` for Hamburg, while every other source has its
`…_processed_measurements` / `…_added_measurements` counters. Those keys are
written by the `on_batch` callback at [`data_import_service.rs:538`](backend/src/core/application/data_import_service.rs:538),
which runs **after** the first batch's `save_batch` and before the `more` check.
⇒ the cancelled statement is executed during the station/channel sync or the
**first `save_batch`** — before any batch of the loop completed.

### 3. The same timeout already kills the rollup job on this host

`GET /api/v1/jobs` contains 11 jobs with SQLSTATE 57014: the 6 Hamburg import
runs **and 5 `measurement_rollup` runs**

| rollup started | duration | result |
|---|---|---|
| 2026-10-31T17:00:00Z | 15.5 s | FAILED 57014 |
| 2026-11-01T00:00:00Z | 41.4 s | FAILED 57014 |
| 2026-11-01T00:15:00Z | 40.5 s | FAILED 57014 |
| 2026-11-01T00:30:00Z | 24.0 s | FAILED 57014 |
| 2026-11-01T03:15:00Z | 19.1 s | FAILED 57014 |
| 2026-11-01T03:30:00Z | 22.6 s | FINISHED (just under the per-statement budget) |

i.e. the heavy `measurement_hourly` / `measurement_daily` /
`measurement_channel_bounds` aggregate statements in [`refresh_rollups`](backend/src/adapter/driven/postgres/measurement_repository.rs:1285)
**legitimately need 15–20 s on this host** and are cut off by the same timeout.
(They are now fast — 0.5 s — because the one-time rollup backfill is complete
and the recent window is small/warm.)

### 4. The timeout is new in 0.0.10 and applies to *every* connection

[`create_pool_with`](backend/src/adapter/driven/postgres/pool.rs:96) puts
`-c statement_timeout=<ms>` into the connection options of the **shared pool**,
so not only the HTTP request path but **also the background import, rollup and
opendata workers** run under it (default `15 s`, plan
[164](plans/164_f1_f5_hardening_plan.md) workstream E1, commit `4f49af2`,
2026-10-31). The deployment switched to 0.0.10 around Nov 1 — exactly when the
import started failing. Before that the same statement simply ran longer than
15 s and completed.

### 5. Why only Hamburg

Hamburg is the only source whose import writes **hundreds of rows per statement**:
302 channels at 5-minute resolution (≈ 11 M rows), so one source-level batch
produces per-channel `save_batch` inserts of up to
`max_measurement_batch_size` rows (1000 in the example config). Every other
source inserts a handful of rows (0–24 on Nov 7; Hessen 568 rows but spread over
589 single-row statements), which stays far below the timeout. Hamburg's backlog
also grows by ~1 day per failed run (1728 rows/channel after 6 days).

### 6. The insert itself is cheap on fast storage — so the host is the variable

Reproduction of the production schema (V1/V5/V14/V15/V20, including the
per-row `measurements_no_overlap` trigger and both indexes) on a 10.4 M-row
synthetic Hamburg-sized table (300 channels × 34 560 five-minute rows):

* trigger lookup plan: `Index Scan Backward using measurements_channel_resolution_time_idx … 0.06 ms`
* `INSERT … ON CONFLICT DO NOTHING` of 1000 rows: **31–47 ms** (warm/cold cache)

⇒ the same statement needs ~15 s on the instance only if the host's storage is
one to two orders of magnitude slower for the cold, random index pages that a
daily import touches (the instance's `data_source_imports` history shows exactly
that: 100 000 rows needed 900 s pre-upgrade). This is consistent with the
rollup-statement timeouts in (3) and is the reason the import hits the ceiling
while the read endpoints stay fast (they are cached/sequential/index-only).

## Root cause

The v0.0.10 request-scoped `statement_timeout` (15 s) is applied to the pooled
connections that the **background import workers** also use. Hamburg's first
non-trivial statement of the run — the first multi-row `save_batch` INSERT into
the ~11 M-row `measurements` table (plus its per-row overlap-guard trigger and
three index updates) — needs more than 15 s on this host's storage. PostgreSQL
cancels it (SQLSTATE 57014), the per-source import run is recorded FAILED, and
because the watermark is only checkpointed **after** a batch is saved, the next
daily run retries the identical statement and fails again. The rollup job
failures on the same host are the same class of problem (heavy aggregate
statements under the same request timeout).

## How to confirm the exact statement (one command on the host)

PostgreSQL logs the cancelled statement at ERROR level by default:

```bash
docker compose logs postgres | grep -B2 -A2 "canceling statement due to statement timeout"
# and
docker compose exec postgres psql -U postgres -d bike_counter -c "SHOW statement_timeout;"
```

Expected: the `INSERT INTO measurements … VALUES (…) …` of the first Hamburg
batch (and the `INSERT INTO measurement_hourly …` of the rollup job).

## Implemented fix

1. **Keep the request guard, exempt the maintenance path.** The new
   `exempt_transaction_from_statement_timeout` helper runs
   `SET LOCAL statement_timeout = 0` as the first statement of the explicit
   transactions in `save_batch` and `refresh_rollups`
   ([`measurement_repository.rs`](backend/src/adapter/driven/postgres/measurement_repository.rs:350)).
   `SET LOCAL` scopes the change to that transaction, so request queries sharing
   the pooled connection keep the configured guard. This fixes both the Hamburg
   import and the rollup-job failures, and also lets a refresh wait for the
   advisory lock held by a competing refresh instead of failing at 15 s.
2. **Bound the per-statement work anyway.** `save_batch` writes the batch in
   `MEASUREMENT_INSERT_CHUNK_ROWS = 250`-row statements inside the same
   transaction, so each statement's cost stays predictable on slow storage and
   the 65 535-parameter cap stays far away.
3. **Documentation.** The README capacity model now states that the statement
   timeout guards the request path while the maintenance transactions lift it.
4. No watermark reset is needed — the first successful run resumes from
   `2026-11-01T03:00Z` and imports the backlog.

## Definition of done

- [x] `save_batch` and `refresh_rollups` are exempt from the request statement
      timeout via transaction-local `SET LOCAL` (repository test asserts
      `current_setting('statement_timeout') = '0'` inside the transaction)
- [x] over-sized measurement batches are chunked; the repository test writes
      `MEASUREMENT_INSERT_CHUNK_ROWS + 3` rows in one batch and asserts all land
- [x] `make check` green (fmt, clippy `-D warnings`, frontend prettier, audit)
- [x] `make test` green (926 passed, including the Postgres repository tests)
- [x] `make coverage` green (backend thresholds and frontend unit coverage)
- [x] README capacity model documents the request-vs-maintenance timeout scope
- [ ] post-deploy verification on the instance (Hamburg import finishes and
      `imported_until` advances past `2026-11-01T03:00:00Z`) — tracked as the
      release follow-up, not a code change
