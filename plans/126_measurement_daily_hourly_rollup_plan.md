# 126 - Measurement rollups for fast graphs and overviews

Status: implemented

## Workflow

All work for this plan is done on a dedicated feature branch
(`feat/measurement-daily-hourly-rollup`) off the current default branch, and
opened as a pull request when complete.

## Context

The station-analytics endpoints (global summary, sidebar/summary `bikes_last_day`,
overview metrics, monthly bar, 30-day/year graphs) all aggregate the raw
[`measurements`](../backend/migrations/V1__create_measurements.sql:14) table on every
request. The covering index from
[`V20`](../backend/migrations/V20__add_measurements_timestamp_index.sql:11) turns
windowed scans into index-only range scans, but the coarse reads still scan a lot
of history:

- [`sum_by_month`](../backend/src/adapter/driven/postgres/measurement_repository.rs:605)
  scans the **entire** history of a channel (Hamburg ~20.9M rows) for the monthly
  bar and all-time total.
- The **year** timeframe re-scans a full year per request, both for its daily
  buckets ([`sum_buckets_by_channel`](../backend/src/adapter/driven/postgres/measurement_repository.rs:382))
  and for its hour-of-day radar
  ([`sum_hours_by_channel`](../backend/src/adapter/driven/postgres/measurement_repository.rs:535)).

On low-IO hardware this makes the initial graph/overview render slow. Caching the
HTTP responses does not help the first load and does not help the per-viewport /
per-timeframe combinations the frontend already fetches on demand.

## Decision

Pre-aggregate measurements into **hourly** and **daily** rollup tables keyed by
`(channel_id, resolution_seconds, local calendar bucket)` and maintain them in the
background. Raw measurements stay the source of truth and keep serving the only
reads the rollups cannot answer: the 5-minute day graph and any custom range finer
than an hour.

- **Daily rollup** serves the scalar overview metrics (day / 7 days / month / year
  are all complete local-day windows), `bikes_last_day`, the monthly bar, the
  all-time total and the 30-day/year daily bucket series.
- **Hourly rollup** serves the hour-of-day radars (incl. the year graph's), whose
  `EXTRACT(HOUR ...)` grouping collapses DST-ambiguous hours exactly like the
  rollup's `local_hour` key.

### Why background maintenance instead of a trigger

The import path
([`save_batch`](../backend/src/adapter/driven/postgres/measurement_repository.rs:131))
inserts up to ~16k rows per statement inside one transaction. A per-row
`AFTER INSERT` trigger would add a channel→station join and an upsert to that hot
path and would serialize bulk imports. A background job (the pattern already used
by [`data_source_update`](../backend/src/core/application/data_source_update_service.rs:34),
[`tiles_update`](../backend/src/core/application/tiles_update_service.rs:40),
[`asset_cleanup`](../backend/src/core/application/asset_cleanup_service.rs:31) and
[`opendata_export`](../backend/src/core/application/opendata_export_service.rs:44))
keeps the import path untouched, and a refresh hook right after each import makes
the rollup fresh immediately after new data lands.

## Design

### 1. Schema (migration `V24__add_measurement_rollups.sql`)

```sql
CREATE TABLE measurement_hourly (
    channel_id         UUID    NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    resolution_seconds BIGINT  NOT NULL,
    local_date         DATE    NOT NULL,
    local_hour         SMALLINT NOT NULL,
    total              BIGINT  NOT NULL,
    PRIMARY KEY (channel_id, resolution_seconds, local_date, local_hour)
);

CREATE TABLE measurement_daily (
    channel_id         UUID   NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    resolution_seconds BIGINT NOT NULL,
    local_date         DATE   NOT NULL,
    total              BIGINT NOT NULL,
    PRIMARY KEY (channel_id, resolution_seconds, local_date)
);

-- Date-first index for the read path, which filters a channel list by a
-- local-date range and then groups by month/hour-of-day.
CREATE INDEX measurement_hourly_date_resolution_idx
    ON measurement_hourly (local_date, resolution_seconds);
CREATE INDEX measurement_daily_date_resolution_idx
    ON measurement_daily (local_date, resolution_seconds);
```

`local_date` / `local_hour` are computed in the **channel's own counting-station
timezone** (each channel belongs to exactly one station with one timezone). This
matches the analytics' DST-aware local-day semantics. The summary endpoints that
aggregate many stations already use a single shared timezone today, so the
per-channel local-date keying stays consistent with the existing behavior; a truly
multi-timezone summary is out of scope.

The migration is **DDL only** — it must not backfill, because a full scan of the
production history (tens of millions of rows) inside a startup migration keeps
the backend in `Starting DB-Migrations` far longer than the healthcheck allows.
The one-time backfill runs in the background in the `measurement_rollup` job
(which reads the measurement bounds and refreshes the whole range on its first
run), so startup never blocks.

`refresh_rollups` widens the requested range by 40 h on both ends so every
station-local calendar day overlapping it is fully covered (a day is at most
24 h; UTC offsets span -12 h…+14 h). It deletes the strictly-interior local days
from the rollup tables and rebuilds them from the raw rows, so a mid-day range
boundary can never leave a partially re-aggregated day behind. The delete only
scans the (small) rollup tables plus the channel/station metadata, and the daily
rollup is rebuilt from the freshly written hourly rows, so the raw measurements
are scanned once per refresh.

Every refresh takes a transaction-scoped `pg_advisory_xact_lock` first. The
scheduled backfill and the post-import hook run concurrently by design, and
without the lock two refreshes can deadlock or re-insert the same bucket between
one transaction's delete and the other's insert (observed in a real deployment).
The lock is released on commit/rollback.

### 2. Repository port and Postgres implementation

Extend the measurement repository port (or add a dedicated rollup port) with
rollup-backed reads, each with an empty default implementation so existing
in-memory test doubles keep compiling:

- `sum_daily(from, to, timezone, channel_ids, resolution) -> i64` — converts the
  UTC window to local dates and sums `measurement_daily`.
- `sum_daily_by_channel(from, to, timezone, channel_ids, resolution) -> Vec<ChannelTotal>`
  — per-channel daily sums (sidebar/summary `bikes_last_day`).
- `sum_daily_buckets_by_channel(from, to, timezone, channel_ids, resolution) -> Vec<ChannelBucket>`
  — per-channel daily bucket series (30-day/year graphs, daily-or-coarser custom
  ranges), with each bucket start as the local-midnight UTC instant.
- `sum_by_month` re-implemented over `measurement_daily` grouped by
  `EXTRACT(YEAR/MONTH FROM local_date)` (replaces the full-history raw scan).
- `sum_hours` / `sum_hours_by_channel` re-implemented over `measurement_hourly`
  grouped by `local_hour` for daily-or-coarser windows (replaces the full-year
  raw scan behind the year graph's hour radar).
- `refresh_rollups(from, to)` — the maintenance write: within one
  advisory-locked transaction, delete the strictly-interior local days of the
  widened range from both rollup tables, re-insert the hourly rows from raw, and
  rebuild the daily rows from the freshly written hourly ones.

### 3. Background job (`MeasurementRollupService`)

A new core service following the existing scheduled-job pattern:

- Job type `measurement_rollup`, ShedLock-style acquire/release, heartbeat and
  cancellation matching [`DataSourceUpdateService`](../backend/src/core/application/data_source_update_service.rs);
  the max heartbeat interval defaults to 300 s so a crashed rollup job is
  reclaimed quickly (the heartbeat loop beats independently of the refresh
  chunks).
- `run_if_due`: if the rollup job has never succeeded, run the full backfill;
  otherwise refresh the last `N` days (3) so a restart or a missed import is
  self-healing. The full backfill walks the history in 7-day chunks (each its own
  advisory-locked transaction), so it commits visible progress, keeps every
  transaction small, and stays cancellable between chunks.
- A `refresh(from, to)` method that the import flow calls directly after each
  successful source import, so the rollup is fresh as soon as data lands without
  waiting for the cron tick. Historical backfills by a provider are covered because
  the refresh uses the imported timestamp range, not a global watermark.

### 4. Read-path switch in the analytics service

- [`metrics::sum_window`](../backend/src/core/application/station_analytics/metrics.rs:32)
  and [`metrics::metric_windows`](../backend/src/core/application/station_analytics/metrics.rs:82)
  call the daily-rollup sums (all four metric windows are complete local days).
- [`bikes_by_station`](../backend/src/core/application/station_analytics/service.rs:289)
  calls `sum_daily_by_channel` for the previous local day.
- [`summary_monthly_totals`](../backend/src/core/application/station_analytics/service.rs:167)
  and `detail_monthly` / `total_bikes` use the rollup-backed `sum_by_month`.
- [`graphs::period_data`](../backend/src/core/application/station_analytics/graphs.rs:426)
  branches on granularity: daily-or-coarser windows use
  `sum_daily_buckets_by_channel` and the rollup-backed hour radar; the 5-minute
  day graph and custom sub-hourly ranges keep the raw `sum_buckets_by_channel`.

The analytics service always picks the **coarsest rollup that can answer the
request exactly**, so it never reads more history than it needs:

| Read | Source |
|---|---|
| Overview metrics day / 7 days / month / year | `measurement_daily` |
| Global / sidebar / summary `bikes_last_day` | `measurement_daily` |
| Monthly bar chart and all-time total | `measurement_daily` |
| 30-day and year daily bucket series | `measurement_daily` |
| Weekday radar over daily-or-coarser buckets | `measurement_daily` |
| Hour-of-day radar incl. the year graph | `measurement_hourly` |
| Week graph hourly buckets | raw `measurements` (bounded to 2 weeks, DST-exact) |
| 5-minute day graph | raw `measurements` |
| Custom range finer than 1 hour | raw `measurements` |

```mermaid
flowchart TD
    A[Import saves raw measurements] --> B[After import: rollup refresh from-to]
    B --> C[measurement_hourly]
    C --> D[measurement_daily]
    E[Scheduler tick] --> B
    F[Analytics read] --> G{coarse window?}
    G -->|yes| D
    G -->|no| H[raw measurements]
```

### 5. Wiring and configuration

- `measurement_rollup_cron` (default `0 */15 * * * *`) and
  `measurement_rollup_max_heartbeat_interval_seconds` (default 300) added to
  [`Configuration`](../backend/src/core/domain/configuration/configuration.rs) with
  a `with_measurement_rollup` builder, validated like the other cron keys, read by
  [`ConfigurationTomlAdapter`](../backend/src/adapter/driven/configuration_toml_adapter.rs)
  and documented in [`config.toml.example`](../config.toml.example).
- `MeasurementRollupService` instantiated in
  [`main.rs`](../backend/src/main.rs:267) and its scheduler spawned inside the
  `scheduled_jobs_enabled` block (so the offline Playwright e2e path is unaffected);
  also registered in the heartbeat
  [reconciliation watcher](../backend/src/core/application/job_reconciliation_service.rs:51).

## Testing strategy (TDD)

Write tests **before** the production code they exercise, following the repo's
existing red/green flow:

1. Rollup read contract: add failing tests against the new port methods using an
   in-memory double, then implement the Postgres repository to satisfy them.
2. [`MeasurementRollupService`](../backend/src/core/application/data_source_update_service.rs)
   behavior (backfill, incremental refresh, heartbeat/cancellation) written against
   a recording job/rollup repository double before the service body exists.
3. Analytics read-path routing: tests asserting that daily-or-coarser windows hit
   the rollup methods and sub-hourly windows still hit the raw methods, written
   before [`graphs.rs`](../backend/src/core/application/station_analytics/graphs.rs)
   and [`metrics.rs`](../backend/src/core/application/station_analytics/metrics.rs)
   are switched over.
4. Postgres repository tests cover the migration backfill, the delete+reinsert
   refresh, and DST-sensitive local-date/local-hour bucketing (23/25-hour days).

### Coverage constraints

- Backend overall production line coverage stays ≥ 80 % and `src/core/` stays
  ≥ 95 % (enforced by `make coverage` / [`scripts/coverage.sh`](../scripts/coverage.sh)).
- The new core service and the new `graphs.rs`/`metrics.rs` routing branches must
  not lower the core threshold; add tests for every new branch instead of
  `#[allow]`-ing coverage.
- No production code ships without its test landing in the same change.

## Out of scope

- HTTP-response caching / Redis (does not address the cold-start scan cost).
- Partitioning the raw measurements table.
- A per-row database trigger on `measurements`.
- Multi-timezone summary correctness (the current code already assumes one shared
  timezone for the aggregated summary views).

## Definition of done

- [x] Failing tests written first for the rollup port contract, rollup service behavior, and analytics routing
- [x] `V24__add_measurement_rollups.sql` added with the tables and indexes (DDL only; the backfill runs in the rollup job)
- [x] Rollup read methods added to the repository port with default impls, satisfying the port-contract tests
- [x] Postgres rollup read/write implementation added and made green by the repository tests
- [x] `MeasurementRollupService` implemented and made green by its behavior tests
- [x] Import flow triggers a rollup refresh after each successful source import
- [x] Analytics metrics, `bikes_last_day`, monthly totals and 30-day/year graphs read from the rollups; routing tests green
- [x] Scheduler + cron configuration wired and validated
- [x] `make check` green
- [x] `make test` green
- [x] `make coverage` green (backend core ≥ 95 %, overall ≥ 80 % — measured 95.00 % / 85.82 %)
- [x] `make test-playwright` green (74 passed; the seeded fixture lacks V23/V24, so they are applied at e2e startup)
- [x] [`README.md`](../README.md) updated where the architecture/README mentions the read model
