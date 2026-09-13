# 128 - Covering indexes for the rollup read path

Status: implemented

## Workflow

Continues the work of
[`plans/126`](126_measurement_daily_hourly_rollup_plan.md) (merged to `main`)
on the branch `fix/rollup-read-indexes`, opened as a pull request when complete.

## Context

After the rollups and the per-channel bounds landed, the Münster station-summary
request (`/summary?...&timeframe=year` with weekly buckets and `compare=1`) still
read about **1.3 GB from disk** and took ~1.9 s (measured with
`pg_stat_reset()` around the request):

```
blks_read | read_mib
  167246  |  1306
```

## Diagnosis

The rollup reads filter `channel_id = ANY($1::uuid[])` and a local-date range but
pass `resolution_seconds = NULL` (the analytics sum every resolution). The two
rollup primary keys are

```
measurement_hourly (channel_id, resolution_seconds, local_date, local_hour)
measurement_daily  (channel_id, resolution_seconds, local_date)
```

so `local_date` is only the **third** column: with the resolution unconstrained
Postgres cannot use the date range for an index seek and instead scans each
requested channel's *whole* history, keeping the one year it needs. The hour
radar alone:

```
measurement_hourly ... channel_id = ANY(...) AND local_date BETWEEN ...
Buffers: shared hit=320292 read=104386   (~815 MiB read, 5.7 s)
```

## Change

Migration `V26__add_rollup_read_indexes.sql` (DDL only):

```sql
CREATE INDEX measurement_hourly_channel_date_idx
    ON measurement_hourly (channel_id, local_date)
    INCLUDE (resolution_seconds, local_hour, total);

CREATE INDEX measurement_daily_channel_date_idx
    ON measurement_daily (channel_id, local_date)
    INCLUDE (resolution_seconds, total);
```

`(channel_id, local_date)` gives a per-channel date range seek; the `INCLUDE`
columns make the aggregate SELECTs index-only (they read `local_hour`/`total`
plus the grouping keys and the optional `resolution_seconds`). This also fixes
the whole-history `sum_by_month`, which now reads the index instead of the heap.

## Verification

Measured on the production-sized dev data (indexes created on the live database):

| query | before | after |
|---|---|---|
| hour radar (one city-year) | 104 386 blocks read / 5.7 s | 6 637 blocks / 0.32 s |
| full summary year/weekly request | 167 246 blocks / **1 306 MiB** / 1.86 s | 1 618 blocks / **12 MiB** / 0.56 s |

No Rust line changes (DDL only), so the production coverage gates are unchanged.

## Definition of done

- [x] `V26__add_rollup_read_indexes.sql` added (DDL only)
- [x] Live `EXPLAIN (ANALYZE, BUFFERS)` before/after captured
- [x] `make check` green
- [x] `make test` green
- [x] `make coverage` green
- [x] `make test-playwright` green
- [x] Committed on `fix/rollup-read-indexes`
