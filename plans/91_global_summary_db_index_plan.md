# 91 - Global summary database index

## Context

`GET /api/bff/global-summary` (the header's whole-system stats) is slow. Its
`bikes_last_day_total` is computed by
[`StationAnalyticsService::global_summary`](../backend/src/core/application/station_analytics/service.rs:391),
which calls
[`bikes_by_station`](../backend/src/core/application/station_analytics/service.rs:289) and
runs one `SUM` query per distinct station timezone over the previous local day.

## Diagnosis

That `SUM` query is
[`PostgresMeasurementRepository::sum_by_channel`](../backend/src/adapter/driven/postgres/measurement_repository.rs:572):

```sql
SELECT channel_id, COALESCE(SUM(value), 0)::bigint AS total
FROM measurements
WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $2 AND timestamp <= $3
  AND ($4::bigint IS NULL OR resolution_seconds = $4::bigint)
GROUP BY channel_id
ORDER BY channel_id
```

The global summary passes every channel id, so `channel_id = ANY(...)` is not
selective; the only selective predicate is the `timestamp` range (the previous
local day). The `measurements` table has indexes whose leading column is
`channel_id` (the natural key from
[`V5`](../backend/migrations/V5__add_measurements_natural_key.sql:16) and the
overlap-guard index from
[`V14`](../backend/migrations/V14__add_measurements_resolution.sql:67)), but none
leading on `timestamp`, so Postgres falls back to a sequential scan over the
whole history for each last-day aggregate. The same helper backs the sidebar and
summary `bikes_last_day` totals, so those endpoints share the cost.

## Change

Add migration `V20__add_measurements_timestamp_index.sql` creating a covering
btree index with `timestamp` as the leading column, so the windowed aggregates
become an index-only range scan:

```sql
-- Covering index for the time-windowed SUM aggregates (global summary,
-- sidebar/summary `bikes_last_day`, graph windows). The leading `timestamp`
-- turns the previous-local-day window into a tight range scan; the INCLUDE
-- columns let Postgres answer SUM(value) GROUP BY channel_id without heap
-- lookups (index-only scan).
CREATE INDEX measurements_timestamp_idx
    ON measurements (timestamp)
    INCLUDE (channel_id, resolution_seconds, value);
```

The migration is discovered automatically by refinery
(`embed_migrations!("migrations")` in
[`pool.rs`](../backend/src/adapter/driven/postgres/pool.rs:20)) and applied once at
startup on a dedicated connection before the pool is handed out.

## Out of scope

- Caching (Redis/Valkey) or denormalizing the summary.
- Partitioning the measurements table.
- Changing `earliest_by_channel` / `latest_by_channel` (already served by the
  `(channel_id, timestamp, resolution_seconds)` unique index).

## Definition of done

- [ ] `V20__add_measurements_timestamp_index.sql` added
- [ ] Plan registered in [`plans/README.md`](../plans/README.md)
- [ ] `make check` green
- [ ] `make test` green (Postgres repository tests run the new migration)
- [ ] Optional: regenerate [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql)
      via `scripts/dump-e2e-fixture.sh` and run `make test-playwright`
