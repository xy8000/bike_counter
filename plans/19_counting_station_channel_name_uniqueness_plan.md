# 19 - Unique counting-station and channel names plan

Status: implemented

## Problem

The imported counting stations and channels contain corrupted data that
violates two naming invariants:

1. A **counting-station name must be unique per data source**. Nothing
   enforces this today: [`counting_stations`](migrations/V1__create_measurements.sql:1)
   only has a primary key on `id`, and the
   [`data_source_id`](../src/core/domain/counting_stations/counting_station.rs:9)
   column added in [`V2`](migrations/V2__add_data_sources.sql:8) has no name
   uniqueness constraint.
2. A **counting station must not have two channels with the same name**. The
   Münster archive genuinely violates this — e.g. `Bohlweg Fahrräder
   Stadteinwärts` appears four times under the `Bohlweg` station (see
   [`SITE_INDEX.md`](../example/radverkehr-zaehlstellen-main/SITE_INDEX.md:17)).
   The parser [`parse_site_index()`](../src/adapter/driven/muenster_github/parsing.rs:31)
   copies channel names verbatim, so duplicates are imported as-is.

The REST layer also hides the data-source relationship: the domain
[`CountingStation`](../src/core/domain/counting_stations/counting_station.rs:2)
carries `data_source_id`, but the
[`CountingStationDto`](../src/adapter/driving/rest/dto/counting_stations.rs:11)
neither exposes that id nor offers a `data_source` HATEOAS link.

## Goal

- Ensure counting-station names are unique **per data source**.
- Ensure channel names are unique **per counting station**.
- When the source data is not already unique (true for Münster channels), the
  Münster adapter appends the external id to the name so it becomes unique.
- Repair already-imported duplicate rows and add database constraints as a
  safety net.
- Expose `data_source_id` on the counting-station Swagger schema and add the
  `data_source` HATEOAS link; review the rest of the Swagger for link gaps.

## Design

### 1. Adapter-level dedup (Münster `parsing.rs`)

Add a small helper in
[`parse_site_index()`](../src/adapter/driven/muenster_github/parsing.rs:31) that
tracks the set of names already used in a scope and appends the external id when
a collision occurs:

```rust
fn unique_name(name: &str, external_id: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        name.to_string()
    } else {
        let candidate = format!("{name} ({external_id})");
        // external ids are unique, so this is unique unless a raw name
        // literally equals it; keep appending defensively.
        let mut final_name = candidate;
        let mut n = 2;
        while !used.insert(final_name.clone()) {
            final_name = format!("{name} ({external_id}#{n})");
            n += 1;
        }
        final_name
    }
}
```

- **Channels**: scope is the counting station. Track one `HashSet` of used
  names per station (keyed by `counting_station_external_id`); the channel
  record's external id is the channel id (already converted to a string).
- **Stations**: scope is the whole archive (which corresponds to one data
  source). Track one `HashSet` across the loop; the station external id is
  `site.directory`.

This keeps dedup in the driven adapter exactly as requested, and the existing
`find_by_external_datasource_id` idempotency in
[`DataImportService`](../src/core/application/data_import_service.rs:104) is
unaffected because `external_id` is never changed.

### 2. Database migration `V8`

A new [`migrations/V8__add_counting_station_and_channel_name_uniqueness.sql`](../migrations/V8__add_counting_station_and_channel_name_uniqueness.sql):

1. **Repair channels** — for every group `(counting_station_id, name)` with
   more than one row, rename all but the first (lowest `id`) by appending
   ` (external_datasource_id)`, falling back to `id::text` when the external id
   is `NULL`:

   ```sql
   UPDATE channels
   SET name = name || ' (' || COALESCE(external_datasource_id, id::text) || ')'
   WHERE id IN (
       SELECT id FROM (
           SELECT id,
                  row_number() OVER (
                      PARTITION BY counting_station_id, name ORDER BY id
                  ) AS rn
           FROM channels
       ) ranked
       WHERE rn > 1
   );
   ```

2. **Repair counting stations** — same treatment scoped per data source, only
   for rows that actually belong to a data source:

   ```sql
   UPDATE counting_stations
   SET name = name || ' (' || COALESCE(external_datasource_id, id::text) || ')'
   WHERE id IN (
       SELECT id FROM (
           SELECT id,
                  row_number() OVER (
                      PARTITION BY data_source_id, name ORDER BY id
                  ) AS rn
           FROM counting_stations
           WHERE data_source_id IS NOT NULL
       ) ranked
       WHERE rn > 1
   );
   ```

3. **Enforce the invariants**:

   ```sql
   CREATE UNIQUE INDEX idx_counting_stations_data_source_id_name
       ON counting_stations (data_source_id, name)
       WHERE data_source_id IS NOT NULL;

   CREATE UNIQUE INDEX idx_channels_counting_station_id_name
       ON channels (counting_station_id, name);
   ```

   The counting-station index is partial because `data_source_id` is nullable
   (unlinked stations are not scoped by any data source). `counting_station_id`
   is `NOT NULL`, so the channel index is full.

The external id is unique per source, so the repaired names are unique within
their scope; if a pathological raw name collided with an appended name, the
`CREATE UNIQUE INDEX` would fail loudly instead of silently corrupting data.

### 3. Swagger / HATEOAS

- [`CountingStationDto`](../src/adapter/driving/rest/dto/counting_stations.rs:11):
  expose the required `data_source_id: Uuid` field (every counting station was
  imported from a data source) and an always-present `data_source` link pointing
  at `/api/v1/data-sources/{id}`.
- Review every DTO under [`src/adapter/driving/rest/dto/`](../src/adapter/driving/rest/dto/mod.rs:7)
  for `_links`. Confirmed already covered: root, channel(s), measurement(s),
  data-source(s), jobs, persistent-state, provider-message. The health DTOs
  ([`health.rs`](../src/adapter/driving/rest/dto/health.rs:1)) and
  [`RawMeasurementDto`](../src/adapter/driving/rest/dto/measurements.rs:52) are
  intentionally link-free (operational / bulk export) and stay that way.

### 4. `data_source_id` backfill for existing rows

Counting stations imported before data-source linking existed have a `NULL`
`data_source_id`, so exposing the field on the DTO revealed that gap. The
requirement that every counting station belongs to a data source is enforced by
the **database**, not the core:

- **Migration `V9`** backfills the link for stations with a `NULL`
  `data_source_id`, guarded to exactly one configured data source (external ids
  are provider-specific and the owning data source is not derivable from a row
  alone), then adds `NOT NULL` on `counting_stations.data_source_id`. The
  original `ON DELETE SET NULL` FK is switched to `ON DELETE CASCADE` (with
  `channels` and `measurements` cascading too), so removing a data source
  removes its whole subtree instead of orphaning stations.
- Because the database guarantees a non-null `data_source_id`, the core needs
  no re-linking logic: the adapter already provides the `data_source_id` on
  every insert.

## File changes

- [`src/adapter/driven/muenster_github/parsing.rs`](../src/adapter/driven/muenster_github/parsing.rs:31)
  — dedup station and channel names by appending the external id.
- [`migrations/V8__add_counting_station_and_channel_name_uniqueness.sql`](../migrations/V8__add_counting_station_and_channel_name_uniqueness.sql)
  — repair existing duplicates + add the two unique indexes (new file).
- [`migrations/V9__backfill_counting_stations_data_source_id.sql`](../migrations/V9__backfill_counting_stations_data_source_id.sql)
  — backfill `data_source_id`, add `NOT NULL`, and switch the FK chain to
  `ON DELETE CASCADE` (new file).
- [`src/adapter/driven/postgres/measurement_repository.rs`](../src/adapter/driven/postgres/measurement_repository.rs:241)
  — Postgres tests now insert a data source and link the station.
- [`src/adapter/driving/rest/dto/counting_stations.rs`](../src/adapter/driving/rest/dto/counting_stations.rs:11)
  — add `data_source_id` field and `data_source` link.
- [`src/adapter/driven/muenster_github/tests.rs`](../src/adapter/driven/muenster_github/tests.rs:208)
  — add parser tests for duplicate channel names and duplicate station names.
- [`src/adapter/driving/rest/tests/dto.rs`](../src/adapter/driving/rest/tests/dto.rs:13)
  — assert the `data_source_id` field and the `data_source` link.
- [`src/adapter/driving/rest/tests/counting_stations.rs`](../src/adapter/driving/rest/tests/counting_stations.rs:8)
  — assert the new field/link on a station that has a data source.
- [`src/adapter/driving/rest/tests/fixtures.rs`](../src/adapter/driving/rest/tests/fixtures.rs:43)
  — optionally add a station fixture with a `data_source_id` (or reuse
  `DATA_SOURCE_ID_A`).
- [`README.md`](../README.md) / [`ToDo.md`](../ToDo.md) — document the new
  uniqueness rules and the `data_source_id`/`data_source` addition.
- [`plans/README.md`](../plans/README.md) — register this plan.

## Acceptance criteria

- [`parse_site_index()`](../src/adapter/driven/muenster_github/parsing.rs:31)
  returns unique channel names per station and unique station names per
  archive; duplicates get ` (external_id)` appended.
- Migration `V8` repairs existing duplicate rows and the two unique indexes are
  created; `make test` (Postgres container) passes.
- `GET /api/v1/counting-stations` / `GET /api/v1/counting-stations/{id}` expose
  `data_source_id` and a `data_source` `_links` entry, always present.
- Previously imported counting stations get a populated `data_source_id`
  (migration `V9`), and `counting_stations.data_source_id` is `NOT NULL` so it
  can never be `null` again.
- `make check`, `make test`, `make test-rest`, and `make coverage` pass.
