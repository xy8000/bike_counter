# 22 - Map view + counting-station GPS coordinates plan

Status: implemented

## Problem

The frontend currently renders "Hello World"
([`frontend/src/App.tsx`](../frontend/src/App.tsx:1)) and only proves the
frontend -> BFF -> backend round-trip works. The user wants a webapp whose
**initial view is a map** showing every counting station.

To place markers on a map, counting stations need GPS coordinates. Today the
archive file the Münster adapter parses —
[`backend/src/adapter/driven/muenster_github/parsing.rs`](../backend/src/adapter/driven/muenster_github/parsing.rs:22)
`RawSite` — has `name`, `directory`, `start` and `channels`, but **no
coordinates**. Coordinates must therefore be provided by the adapter, persisted,
exposed over the API, and rendered by the frontend.

## Goal

- Counting stations carry **optional** GPS coordinates.
- The Münster adapter provides coordinates from a **hardcoded** lookup table
  keyed by the station's external id; stations not in the table have "not
  provided" (`None`) coordinates.
- The import **syncs** stations by external id: it updates existing stations
  (name, description, coordinates) and inserts new ones.
- A **PATCH** endpoint updates a station's coordinates.
- The REST API exposes coordinates; Swagger documents the new endpoint/fields.
- The frontend's first view is a map (Leaflet) with one marker per station that
  has coordinates.

## Decisions (clarified)

1. **Coordinates are optional** — modeled as `Option<GeoCoordinates>`.
2. **Coordinate source = a hardcoded table inside the Münster adapter**, keyed by
   the station external id. Stations not listed get `None`. The table also
   carries a canonical display name that the adapter overlays onto the station.
3. **No dropping of coordinate-less stations** — they remain persisted with `None`
   coordinates and can be patched later.
4. **Sync is an upsert keyed by external id** — existing stations are updated
   (name, description, coordinates) on the next sync; new stations are inserted.
5. **PATCH endpoint** for coordinates, implemented through a core service (the
   same pattern as the `persistent_state` write endpoints), not a direct
   repository call.
6. **Leaflet + OpenStreetMap tiles** (free, no API key) via `react-leaflet`.
7. **Frontend consumes the existing `GET /api/v1/counting-stations`** (coordinates
   are a natural property of the resource; no BFF endpoint needed).
8. **Storage: two nullable `DOUBLE PRECISION` columns** (`latitude`, `longitude`).
   PostGIS `geography(Point, 4326)` was considered and rejected for now (it would
   require a PostGIS image, the `postgis` crate, and an extension migration). Two
   double columns are sufficient for map markers and can be migrated to PostGIS
   later if server-side spatial queries become necessary.

## Hardcoded station metadata (source data)

The adapter hardcodes this table (`external_id -> name, latitude, longitude`):

```text
300038855,Bismarckallee,51.9565,7.6152
300037926,Bohlweg,51.9688,7.6432
300039328,Coesfelder Kreuz,51.9662,7.6006
100034978,Gartenstraße,51.9701,7.6358
300037931,Gasselstiege,51.9772,7.6125
300037925,Goldstraße,51.9612,7.6305
300039331,Grevener Straße,51.9754,7.6189
100031300,Hafenstraße,51.9548,7.6323
100034980,Hammer Straße,51.9546,7.6258
100034982,Hüfferstraße,51.9623,7.6098
300037544,Kanalpromenade Abschnitt 1 (Dingstiege),51.9902,7.6410
100053305,Kanalpromenade Abschnitt 5,51.9424,7.6612
300037936,Kanalpromenade Abschnitt 6,51.9185,7.6789
300037928,Kinderhauser Str.,51.9731,7.6212
300037920,Lütkenbecker Str.,51.9429,7.6485
100035541,Neutor,51.9673,7.6184
100031297,Promenade (nördlich Salzstraße),51.9617,7.6335
300037405,Promenade (westlicher Hals),51.9589,7.6195
300037932,Schmeddingstraße,51.9511,7.6012
100034983,Warendorfer Straße,51.9613,7.6410
300037933,Weißenburg Str.,51.9482,7.6318
100034981,Weseler Straße,51.9516,7.6160
100020113,Wolbecker Straße,51.9568,7.6397
```

## Design

### 1. Hardcoded station metadata (Münster adapter)

Add a new module
[`backend/src/adapter/driven/muenster_github/station_metadata.rs`](../backend/src/adapter/driven/muenster_github/station_metadata.rs)
holding the table above and a lookup, e.g.
`metadata_for(external_id: &str) -> Option<StationMetadata>`
with `StationMetadata { name: String, latitude: f64, longitude: f64 }`.

Register it in
[`backend/src/adapter/driven/muenster_github/mod.rs`](../backend/src/adapter/driven/muenster_github/mod.rs:1).

In [`.../adapter.rs`](../backend/src/adapter/driven/muenster_github/adapter.rs:503)
`get_all_counting_stations`, after `parse_site_index` builds the records, overlay
the metadata: for each station, if the external id is in the table, set
`latitude`/`longitude` and override `name` with the canonical name; otherwise
leave coordinates as `None`. (Table names are assumed unique; the existing
archive-name dedup still runs first.)

### 2. Domain + provider port

In
[`backend/src/core/domain/counting_stations/counting_station.rs`](../backend/src/core/domain/counting_stations/counting_station.rs:1):

- Add a `GeoCoordinates` value object (`latitude: f64`, `longitude: f64`;
  `Copy`, `Clone`, `Debug`).
- Add `pub coordinates: Option<GeoCoordinates>` to `CountingStation`.

In
[`backend/src/core/domain/data_source/provider_port.rs`](../backend/src/core/domain/data_source/provider_port.rs:88):

- Add `pub latitude: Option<f64>` and `pub longitude: Option<f64>` to
  `CountingStationRecord`.

### 3. Persistence

Add `backend/migrations/V10__add_counting_station_coordinates.sql`:

```sql
ALTER TABLE counting_stations ADD COLUMN latitude DOUBLE PRECISION;
ALTER TABLE counting_stations ADD COLUMN longitude DOUBLE PRECISION;
```

Update the repository port
[`backend/src/core/domain/counting_stations/repository_port.rs`](../backend/src/core/domain/counting_stations/repository_port.rs:1)
with an `update(&self, station: CountingStation) -> Result<(), DomainError>`
method (upsert of name/description/coordinates by id).

Update
[`backend/src/adapter/driven/postgres/counting_station_repository.rs`](../backend/src/adapter/driven/postgres/counting_station_repository.rs:1):

- `map_row` reads the two optional columns into `Option<GeoCoordinates>`.
- `save` inserts `latitude`/`longitude`.
- New `update` executes
  `UPDATE counting_stations SET name = $2, description = $3,
  external_datasource_id = $4, data_source_id = $5, latitude = $6,
  longitude = $7 WHERE id = $1`.
- Update the `SELECT` column lists in `find_by_id`, `find_all`,
  `find_by_external_datasource_id`, and `find_filtered`.

Update every in-memory repository mock to implement the new `update` method.

### 4. Sync (upsert) in DataImportService

In
[`backend/src/core/application/data_import_service.rs`](../backend/src/core/application/data_import_service.rs:104)
`sync_counting_stations`:

- Build the coordinates from the record (`latitude`/`longitude` both present
  -> `Some(GeoCoordinates)`, otherwise `None`).
- If a station with the external id already exists, **update** it when
  name/description/coordinates differ (via the repository `update`); do not
  count it as new.
- Otherwise insert a new station (with optional coordinates) and increment
  `summary.counting_stations`.
- Keep returning the `external_id -> station UUID` map for channel linking.

Update the import-service tests: add cases for coordinate upsert (existing
station updated, new station inserted, missing coordinates -> `None`).

### 5. REST API

In
[`backend/src/adapter/driving/rest/dto/counting_stations.rs`](../backend/src/adapter/driving/rest/dto/counting_stations.rs:10):

- Add `latitude: Option<f64>` and `longitude: Option<f64>` to
  `CountingStationDto`, populated from the entity.
- Add a `CountingStationPatchDto { latitude: Option<f64>, longitude: Option<f64> }`
  (`Deserialize`, `ToSchema`) — fields optional; `null` clears a coordinate.

Add a service method
[`CountingStationService::update_coordinates`](../backend/src/core/application/counting_station_service.rs:22)
(resolve station, set coordinates, call repository `update`) and expose it via
the driving port
[`backend/src/core/domain/counting_stations/service_port.rs`](../backend/src/core/domain/counting_stations/service_port.rs:1).

Add a handler
[`backend/src/adapter/driving/rest/handlers/counting_stations.rs`](../backend/src/adapter/driving/rest/handlers/counting_stations.rs:40):

- `PATCH /api/v1/counting-stations/{id}` accepting `CountingStationPatchDto`,
  annotated with `#[utoipa::path(..., tag = "Counting Stations")]`, returning
  the updated `CountingStationDto` (or `404`/`500`).

Register the route in
[`backend/src/adapter/driving/rest/mod.rs`](../backend/src/adapter/driving/rest/mod.rs:70)
and the handler/DTO in
[`backend/src/adapter/driving/rest/openapi.rs`](../backend/src/adapter/driving/rest/openapi.rs:1)
so Swagger documents it.

Update the REST tests/fixtures
([`backend/src/adapter/driving/rest/tests/counting_stations.rs`](../backend/src/adapter/driving/rest/tests/counting_stations.rs:1))
for the new fields and the PATCH endpoint.

### 6. Frontend — Leaflet map view

In [`frontend/`](../frontend):

- Add dependencies: `leaflet`, `react-leaflet`, and dev `@types/leaflet`; import
  the Leaflet CSS (in [`frontend/src/index.css`](../frontend/src/index.css:1) or
  [`main.tsx`](../frontend/src/main.tsx:1)).
- Replace [`frontend/src/App.tsx`](../frontend/src/App.tsx:1) with a map view:
  - On mount, `fetch('/api/v1/counting-stations')`.
  - Center on Münster (about `51.96, 7.63`) with a sensible zoom.
  - Render one `Marker` per station in `items` where both `latitude` and
    `longitude` are present; skip `null`-coordinate stations defensively.
  - `Popup` shows the station name.
  - Loading and error states (mirroring the current BFF fetch pattern).
- Keep the `npm run dev` proxy and nginx `/api` reverse proxy unchanged
  (same-origin, no CORS).

### 7. Backfill via the next sync

No manual data reset is required. Because `sync_counting_stations` now **updates**
existing stations, the next scheduled data-source update (hourly cron, or the
startup catch-up when overdue) backfills coordinates onto already-imported
stations keyed by external id.

## Out of scope

- A dedicated BFF map/aggregation endpoint (the public REST endpoint suffices).
- Clicking a marker to drill into channels/measurements (name popup only).
- Custom map tiles / branded basemap (plain OpenStreetMap tiles).
- PATCHing `name`/`description` (coordinates only; name/description are synced
  from the adapter).
- Deleting stations that disappear from the source (update/insert only).
- Enforcing `NOT NULL` on the coordinate columns.

## Result

Implemented end-to-end:

- [`station_metadata.rs`](../backend/src/adapter/driven/muenster_github/station_metadata.rs:1)
  holds the hardcoded table and overlays name + coordinates in
  [`adapter.rs`](../backend/src/adapter/driven/muenster_github/adapter.rs:390).
- `GeoCoordinates` + optional `coordinates` on
  [`counting_station.rs`](../backend/src/core/domain/counting_stations/counting_station.rs:1);
  optional `latitude`/`longitude` on `CountingStationRecord`.
- Migration
  [`V10__add_counting_station_coordinates.sql`](../backend/migrations/V10__add_counting_station_coordinates.sql:1).
- Repository `update` (name/description/coordinates) + coordinate read/write.
- `DataImportService::sync_counting_stations` upserts by external id.
- `PATCH /api/v1/counting-stations/{id}` (Swagger-documented) +
  `CountingStationService::update_coordinates`.
- Frontend Leaflet map view in [`App.tsx`](../frontend/src/App.tsx:1).

Gates green: `make check`, `make test` (235), `make test-rest`,
`make coverage` (overall 84.67%, core 96.56%), `make frontend-build`.

## Testing / gates

- Backend: unit tests for the metadata overlay (listed id gets name+coords,
  unlisted id gets `None`), the sync upsert (update existing / insert new),
  repository `update` round-trip, service `update_coordinates`, and REST DTO +
  PATCH mapping; update fixtures.
- Frontend: `make frontend-build` (TypeScript compiles; markers render in the
  running stack).
- Run [`make check`](../Makefile:28), [`make test`](../Makefile:31),
  [`make test-rest`](../Makefile:34), [`make coverage`](../Makefile:42), and
  [`make frontend-build`](../Makefile:43) — all must be green.
- Manual check: `make run`, open <http://localhost:8081>, and verify a marker per
  listed Münster counting station on the map.
