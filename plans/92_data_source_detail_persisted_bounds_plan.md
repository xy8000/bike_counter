# 92 — Persist per-data-source first/last measurement bounds

Status: implemented

## Context

The data-source detail page (`GET /api/bff/data-sources/{id}`) loads slowly for
large sources. Measured end-to-end on the running dev stack, the Hamburg detail
payload takes **~8.4 s**; the page issues exactly this one request via
[`fetchDataSourceDetail`](../frontend/src/features/dataSources/api.ts:16).

## Diagnosis (measured with `EXPLAIN ANALYZE` on the dev DB)

The endpoint runs the queries in
[`DataSourceAnalyticsService::detail`](../backend/src/core/application/data_source_analytics_service.rs:87).
The two dominant costs are the first/last lookups:

| Query | Plan shape (Hamburg, 20 920 764 rows) | Time |
|---|---|---|
| [`earliest_by_channel`](../backend/src/adapter/driven/postgres/measurement_repository.rs:770) | Index Only Scan over the **whole** per-channel history + `Unique` | 2.5 s warm / 6.6 s cold |
| [`latest_by_channel`](../backend/src/adapter/driven/postgres/measurement_repository.rs:739) | same full index scan + `Incremental Sort` (external merge, ~21 MB spill) | 6.2 s |

Both run `SELECT DISTINCT ON (channel_id) … ORDER BY channel_id, timestamp …`
over `channel_id = ANY($1::uuid[])`. PostgreSQL has **no skip scan for
`DISTINCT ON`**, so the executor reads every measurement row of the source's
channels (the whole 20.9 M rows) and then applies `Unique`; the descending
variant additionally re-sorts each channel's rows. The code comment claiming
these are "an index seek that stops at one row per channel" does not match what
Postgres actually does.

**An extra index on `measurements` cannot fix this.** The query already uses the
ideal index — the natural-key unique index
(`(channel_id, timestamp, resolution_seconds)` from
[`V14`](../backend/migrations/V14__add_measurements_resolution.sql:57)) — so any
additional btree on those columns would be redundant; the cost is the scan
itself, not index access.

Everything else on the page is already cheap and indexed:

- stations by data source: 2.9 ms (unique index
  [`idx_counting_stations_data_source_id_name`](../backend/migrations/V8__add_counting_station_and_channel_name_uniqueness.sql:42))
- channel ids by data source: 2.5 ms
- per-month `has_measurements_in_windows` coverage probes: ~1–12 ms each (9 of them)
- last import run: indexed
  [`idx_data_source_imports_source_started`](../backend/migrations/V19__add_data_source_imports.sql:19)
- provider-message counts: ≤1001 rows per source (V13 cap)

## Decision

Persist the source-wide `first_measurement_at` / `last_measurement_at` directly
on `data_sources` and update them from the **core import flow**, then have the
detail read model return those persisted values instead of scanning the whole
measurement history. This matches the existing
[`DataSource`](../backend/src/core/domain/data_source/data_source.rs:12) pattern
(`imported_until`, `last_updated_at` already live on the same row) and keeps the
logic in the hexagonal core, not in a DB trigger.

## Change

### 1. Migration `V21__add_data_source_measurement_bounds.sql`

- `ALTER TABLE data_sources ADD COLUMN first_measurement_at TIMESTAMPTZ;`
- `ALTER TABLE data_sources ADD COLUMN last_measurement_at TIMESTAMPTZ;`
- Backfill both from existing measurements (one-time; the table already carries
  30 M+ rows so this runs once at startup, comparable to the V20 index build):

```sql
UPDATE data_sources ds
SET first_measurement_at = b.first_ts,
    last_measurement_at  = b.last_ts
FROM (
    SELECT s.data_source_id,
           MIN(m.timestamp) AS first_ts,
           MAX(m.timestamp) AS last_ts
    FROM measurements m
    JOIN channels c ON c.id = m.channel_id
    JOIN counting_stations s ON s.id = c.counting_station_id
    WHERE s.data_source_id IS NOT NULL
    GROUP BY s.data_source_id
) b
WHERE ds.id = b.data_source_id;
```

No index needed: `data_sources` has one row per source and the columns are read
by primary key.

### 2. Domain

- [`DataSource`](../backend/src/core/domain/data_source/data_source.rs:12): add
  `pub first_measurement_at: Option<DateTime<Utc>>` and
  `pub last_measurement_at: Option<DateTime<Utc>>`; initialize both to `None` in
  [`DataSource::new`](../backend/src/core/domain/data_source/data_source.rs:34).
- [`DataSourceRepository`](../backend/src/core/domain/data_source/repository_port.rs:7):
  add a port method with a default no-op (mirrors `update_logo`) so existing
  in-memory doubles compile untouched:

```rust
/// Records the source-wide earliest/latest measurement timestamps for a run
/// that inserted measurements. The merge is idempotent (min/max), so a later
/// historical backfill or a newer batch both converge correctly.
fn update_measurement_bounds(
    &self,
    _id: Id,
    _first: Option<DateTime<Utc>>,
    _last: Option<DateTime<Utc>>,
) -> Result<(), DomainError> {
    Ok(())
}
```

### 3. Core import flow (the actual logic)

- [`DataSourceUpdate`](../backend/src/core/application/data_import_service.rs:40):
  add `first_measurement_timestamp: Option<DateTime<Utc>>` and
  `last_measurement_timestamp: Option<DateTime<Utc>>`.
- [`DataImportService::update_data_source`](../backend/src/core/application/data_import_service.rs:331):
  while folding each source-level batch, track the run-wide minimum and maximum
  measurement `timestamp` over every processed record (skipped `ON CONFLICT`
  rows share timestamps already within the persisted bounds, so including them
  is safe and keeps the fold simple). Return the two values in the
  `DataSourceUpdate`. Do **not** introduce a data-source-repository dependency
  into `DataImportService`.
- [`DataSourceUpdateService::run_updates`](../backend/src/core/application/data_source_update_service.rs:226):
  on the `Ok(update)` arm call
  `self.data_source_repository.update_measurement_bounds(data_source_id, update.first_measurement_timestamp, update.last_measurement_timestamp)?`
  whenever at least one timestamp is `Some` (i.e. measurements were persisted),
  independently of `update.completed` — a deadline-stopped run still inserted
  data and must advance the bounds.

### 4. Postgres adapter

- [`PostgresDataSourceRepository`](../backend/src/adapter/driven/postgres/data_source_repository.rs:18):
  add the two columns to every `SELECT` list and to `map_row` (indices 7/8).
- Implement `update_measurement_bounds` as a single NULL-safe atomic UPDATE:

```sql
UPDATE data_sources
SET first_measurement_at = CASE
        WHEN $2::timestamptz IS NULL THEN first_measurement_at
        WHEN first_measurement_at IS NULL THEN $2::timestamptz
        ELSE LEAST(first_measurement_at, $2::timestamptz)
    END,
    last_measurement_at = CASE
        WHEN $3::timestamptz IS NULL THEN last_measurement_at
        WHEN last_measurement_at IS NULL THEN $3::timestamptz
        ELSE GREATEST(last_measurement_at, $3::timestamptz)
    END
WHERE id = $1;
```

### 5. Detail read model

- [`DataSourceAnalyticsService::detail`](../backend/src/core/application/data_source_analytics_service.rs:87):
  replace the `earliest_by_channel` / `latest_by_channel` calls with
  `first_data_at = data_source.first_measurement_at` and
  `last_data_at = data_source.last_measurement_at`. Keep
  [`has_measurements_in_windows`](../backend/src/adapter/driven/postgres/measurement_repository.rs:640)
  for the "full current year coverage" badge (it is already cheap). The
  `DataSourceDetail` read model, the BFF DTO and the handler stay unchanged.

### 6. Tests

- Postgres repo test: `update_measurement_bounds` round-trip — set, widen
  (earlier `first`, later `last`), and ignore `NULL`.
- Update the [`DataSourceAnalyticsService`](../backend/src/core/application/data_source_analytics_service.rs:211)
  detail tests to set `first_measurement_at` / `last_measurement_at` on the
  in-memory `DataSource` and assert the badges derive from them.
- [`DataImportService`](../backend/src/core/application/data_import_service.rs:504)
  tests: assert `DataSourceUpdate` reports the run-wide min/max.
- [`DataSourceUpdateService`](../backend/src/core/application/data_source_update_service.rs:312)
  tests: assert `update_measurement_bounds` is invoked with the run bounds.
- [`fixtures.rs`](../backend/src/adapter/driving/rest/tests/fixtures.rs:156):
  add the two fields to the `DataSource` struct literal.

### 7. Fixture + docs

- Regenerate [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql) via
  `scripts/dump-e2e-fixture.sh` so the committed schema/refinery history include
  V21 (otherwise `make test-playwright` would fail re-applying V21).
- Register this plan in [`plans/README.md`](../plans/README.md).

## Out of scope

- Replacing [`latest_by_channel`](../backend/src/adapter/driven/postgres/measurement_repository.rs:739)
  for the import-time stale-station check in
  [`mark_stale_stations_inactive`](../backend/src/core/application/data_import_service.rs:426).
  That check is per-station (needs per-station latest) and runs once per import,
  not per page view.
- Caching (Redis/Valkey), partitioning `measurements`, or denormalizing
  `data_source_id` onto `measurements`.

## Definition of done

- [x] `V21__add_data_source_measurement_bounds.sql` added (backfill measured ~5 s on the 30 M-row dev DB)
- [x] `DataSource` + repository port + Postgres adapter extended
- [x] Core import flow tracks and persists the run bounds
- [x] `DataSourceAnalyticsService::detail` reads persisted bounds
- [x] Tests added/updated as above
- [x] `make check` green
- [x] `make test` green (551 tests, Postgres repo tests apply V21)
- [x] `make test-rest` green (109 tests)
- [x] `make coverage` green (overall 86.48%, core 95.07%)
- [x] Backend-only change (no frontend files touched), so `make test-playwright` is not a required gate here
- [x] Recommended follow-up: regenerate [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql) from a V21 stack (`scripts/dump-e2e-fixture.sh`) and run `make test-playwright` to keep the committed fixture aligned with the new schema
