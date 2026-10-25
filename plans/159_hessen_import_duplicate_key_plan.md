# 159 - Hessen import duplicate-key fix

Status: implemented

## Goal

Fix the Hessen import failure reported in GitHub issue #37:

```
duplicate key value violates unique constraint "idx_counting_stations_data_source_id_name"
Key (data_source_id, name)=(..., 1530) already exists.
```

## Root cause

[`DataImportService::sync_counting_stations`](backend/src/core/application/data_import_service.rs)
matches stations by `external_datasource_id`. When the provider re-issues a
short code under a **new** external id (but keeps the old name), the lookup
misses and the service `INSERT`s a station whose name already exists for that
data source, violating the unique `(data_source_id, name)` index (created in
[`V8__add_counting_station_and_channel_name_uniqueness.sql`](backend/migrations/V8__add_counting_station_and_channel_name_uniqueness.sql)).
The scraping adapter's `ensure_unique_names` only disambiguates names *within* a
single batch, so it cannot see the previously persisted collision.

## Fix

In [`data_import_service.rs`](backend/src/core/application/data_import_service.rs):

- Seed a `used_names` set from every station already persisted for the data
  source.
- When inserting a new station whose name is already used, reserve a stable
  disambiguated name (`"{name} ({external_id})"`, then ` #2`, ` #3`, …) instead
  of failing the `INSERT`.

A regression test (`reuses_a_name_under_a_new_external_id_without_colliding`)
covers the exact "1530 re-issued under a new id" scenario.

## Definition of done

- [x] New stations get a stable, collision-free name
- [x] Regression test added and passing
- [x] `cargo fmt` + `cargo clippy --all-targets -- -D warnings` green
