# 156 - Hessen Mobil import: surface the real PostgreSQL error behind `Database("db error")`

Status: implemented

## Problem

The `Hessen Mobil` data source (`eco_counter_web_http_provider`) reports

```
The last import failed: Database("db error")
```

`Database("db error")` is the Debug rendering of
`DomainError::Database("db error")`. The inner `"db error"` string is produced by
every Postgres repository mapping a query failure with
`error.to_string()`. For the synchronous `postgres` crate (which re-exports
`tokio_postgres::Error`) the `Display` impl of a `Kind::Db` error is the literal
`"db error"` — see
[`tokio-postgres 0.7.18 error/mod.rs`](https://docs.rs/tokio-postgres/0.7.18) —
so the **actual** PostgreSQL error (SQLSTATE, message, constraint name) is
thrown away and the UI/logs cannot tell a unique-violation from a type error.

### Root cause of the import failure itself (already fixed on `main`)

The import fails on a uniqueness violation during station persistence: the
Hessen dashboard lists **two** sites with the short code `1394` (external ids
`300066740` / `300071542`), and the unique index
[`idx_counting_stations_data_source_id_name`](../backend/migrations/V8__add_counting_station_and_channel_name_uniqueness.sql:42)
rejects the second `counting_stations` insert. That root cause was fixed in
plan 152 (commits `6412323`, `e6245f7`, `0d0e8dd`, already on `main`):
[`parse_site_list`](../backend/src/adapter/driven/eco_counter/scraping/parsing.rs:241)
disambiguates colliding names via
[`ensure_unique_names`](../backend/src/adapter/driven/eco_counter/scraping/parsing.rs:339)
and the persisted index is normalised on load
([`decode_index`](../backend/src/adapter/driven/eco_counter/scraping/adapter.rs:633)).
Verified against the live payload (2026-10-24): 554 sites, 554 unique external
ids, 553 unique names — exactly one duplicate (`1394`), which the parser
disambiguates to 554 unique names.

Two gaps remain and are fixed here:

1. **The opaque error mapping.** Any future DB failure is still reported as the
   useless `Database("db error")`, so a regression is undiagnosable. All
   Postgres repositories must surface the real DB error.
2. **Stale channel names.** `sync_counting_stations` refreshes a renamed
   station's name, but
   [`sync_channels`](../backend/src/core/application/data_import_service.rs:311)
   only inserts new channels and never refreshes an existing channel's name, so
   a disambiguated station (e.g. `1394 (300066740)`) keeps a channel still named
   `1394`. The channel never follows the station rename.

## Fix

1. **Surface the real DB error.** Add a shared helper in
   [`pool.rs`](../backend/src/adapter/driven/postgres/pool.rs) that renders a
   `postgres::Error` as `"<message> (SQLSTATE <code>)"` and appends
   `constraint`/`detail`/`table`/`column` when present, falling back to
   `to_string()` for non-DB errors. Replace the `error.to_string()` mappings in
   the Postgres repositories with this helper so the failure message on the
   import run carries the actual PostgreSQL diagnostic.
2. **Refresh existing channel names.** Add `ChannelRepository::update` (required
   method) and implement it in `PostgresChannelRepository` and the in-test
   doubles. In `sync_channels`, when an existing channel's
   name/description/station link differs from the provider's record, update it —
   mirroring how `sync_counting_stations` refreshes stations.
3. **Fix two pre-existing Playwright e2e failures** that broke the hard gate:
   - [`settings.spec.ts`](../frontend/e2e/settings.spec.ts) — the "individual
     range" test hardcoded `2026-08-25..31`, which drifted out of the fixture's
     rolling data window (Münster spans `now()-45d … now()`), so the graph
     returned zero buckets. The range is now computed relative to today and
     spans exactly seven days so it keeps the week resolution set (mid = hour).
   - [`url.spec.ts`](../frontend/e2e/url.spec.ts) — the map void-click can land
     on the just-opened station popup (a guarded, non-void target) while the
     marker-click zoom animation is still settling. The void click is now
     retried via `expect(...).toPass(...)` until the `station` param is dropped.

## File changes

Modified:

- `backend/src/adapter/driven/postgres/pool.rs` — add `db_error` helper.
- `backend/src/adapter/driven/postgres/*_repository.rs` — use the helper instead
  of `error.to_string()`.
- `backend/src/core/domain/channels/repository_port.rs` — add `update`.
- `backend/src/adapter/driven/postgres/channel_repository.rs` — implement
  `update`.
- `backend/src/core/application/data_import_service.rs` — refresh existing
  channel names/descriptions in `sync_channels`; update the in-test mock.
- `backend/src/core/application/data_source_update_service.rs`,
  `backend/src/core/application/data_source_analytics_service.rs`,
  `backend/src/core/application/channel_service.rs`,
  `backend/src/core/application/station_analytics/tests.rs`,
  `backend/src/adapter/driving/rest/tests/mocks.rs` — update in-test `ChannelRepository` doubles.
- `frontend/e2e/settings.spec.ts` — relative seven-day individual range.
- `frontend/e2e/url.spec.ts` — retried map void-click.
- `plans/156_hessen_import_db_error_plan.md` — this plan.

No DB migration, no config change.

## Testing

- Unit: `db_error` renders `message` + `SQLSTATE` + `constraint` for a Db error
  and falls back to `to_string()` otherwise.
- Unit: `sync_channels` updates an existing channel whose name changed and keeps
  an unchanged channel untouched.
- Existing scraping tests keep passing (they already pin the `1394`
  disambiguation and persisted-index normalisation).
- Playwright e2e: the two previously failing specs were reproduced against a
  booted stack, root-caused (fixture date drift + map popup race), fixed, and the
  full suite re-run green.

## Acceptance criteria

- A Postgres constraint failure is reported with its SQLSTATE, message and
  constraint name instead of `Database("db error")`.
- `sync_channels` refreshes an existing channel's name to match a renamed
  station.
- `make check`, `make test-rest`, `make test`, `make test-playwright` green.

## Definition of done

- [x] Plan file added
- [x] `db_error` helper added and used across the Postgres repositories
- [x] `ChannelRepository::update` added and implemented
- [x] `sync_channels` refreshes changed channel names
- [x] Unit tests added/updated
- [x] `make test` (835 tests) and `make test-rest` (128 tests) green
- [x] `make check` green for fmt + clippy + prettier (`scripts/fmt-test.sh`, `npm run format:check`)
- [x] `make test-playwright` green (74 passed)
- [x] Docs/plan updated

## Gate note

`make check`'s final `cargo audit` step fails on **10 pre-existing RustSec
advisories** (published 2026-10-09 … 2026-10-20 against `rustls`, `zip`,
`tokio`, `webpki-roots`, `yoke-derive`) that are unrelated to this change and
reproduce on `main`. Resolving them is a dependency-upgrade follow-up, not part
of this fix.
