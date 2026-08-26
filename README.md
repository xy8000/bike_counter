# Bike-Counter

This Repository can be used to analyse the Bike-Counter-Stations of Münster.

It is a **monorepo** with two sub-projects:

- [`backend/`](backend) — Rust (Axum, hexagonal architecture) REST API backed by
  a PostgreSQL database, documented via auto-generated OpenAPI and browsable
  through Swagger-UI. It also exposes a **Backend-for-Frontend (BFF)** API under
  `/api/bff` that is consumed by the frontend **only** and appears in Swagger
  under its own `BFF API` collection.
- [`frontend/`](frontend) — React (Vite) single-page application served by nginx
  in the Docker stack: a Leaflet map with one marker per counting station that
  has GPS coordinates, a left sidebar listing the stations currently visible in
  the viewport (name, description, channel count, bikes in the last 24 h), a
  Komoot-style header with a search dialog, a live aggregate summary, a
  per-station detail page (`/stations/:id`) with the overview stat boxes (incl.
  the YEAR stat), a shared timeframe selector (24 hours / current + last week /
  last 30 days / last year) driving a full-width line chart, a weekday radar,
  the channel pie and the per-channel nerd stats (plus a "compare previous
  period" checkbox that overlays the previous period), and a standalone monthly
  bar chart showing the grand total. A **station summary page** (`/summary`,
  opened from the sidebar's "Summarize visible stations" button) aggregates the
  stations currently visible in the map view — fallback image, an interactive map
  (click a flag to exclude a station, grayed out), the aggregated overview stats
  and the same charts with per-**station** nerd stats. The map view + disabled
  stations are encoded in the URL (`min_lat`/`min_lng`/`max_lat`/`max_lng` +
  `disabled`), so a summary can be shared and restored; the page shows a loading
  state while the backend aggregates (caching is planned later).

Docker Compose ramps up the whole stack (`db` + `backend` + `frontend`).

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain, edition 2024)
- [Node.js](https://nodejs.org) 24 LTS (for the React frontend; pinned in
  [`frontend/.nvmrc`](frontend/.nvmrc) — Vite 7 needs at least Node 20.19/22.12)
- A running PostgreSQL server (see [Configuration](#configuration) below)

## Configuration

All settings live in [`config.toml`](config.toml) (gitignored). Start from the
tracked template [`config.toml.example`](config.toml.example):

```bash
cp config.toml.example config.toml
```

```toml
database_url="postgres://db:5432"
database_user="postgres"
database_password="postgres"
database_name="bike_counter"

# CRON expression for the data-source update job (default: every hour).
data_source_update_cron="0 * * * * *"
# REQUIRED ShedLock-style max lifetime for the update job in seconds (no default).
data_source_update_max_lifetime_seconds=600

# CRON expression for the asset cleanup job (default: daily at 04:00).
asset_cleanup_cron="0 0 4 * * *"
# REQUIRED ShedLock-style max lifetime for the asset cleanup job in seconds (no default).
asset_cleanup_max_lifetime_seconds=3600

# S3-compatible object storage (MinIO) holding counting-station image binaries.
# PostgreSQL stores only the metadata; the BFF streams the content to browsers.
[asset_storage]
endpoint = "http://minio:9000"
access_key = "minioadmin"
secret_key = "minioadmin"
bucket = "bike-counter-images"
region = "us-east-1"

[[data_sources]]
name = "Münster"

[data_sources.provider]
type = "münster_opendata_github_provider"

[data_sources.provider.vars]
url = "https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"
max_measurement_batch_size = "500"
# Import time window per provider call, in hours (default 7 days = 168).
max_measurement_timeframe_hours = "168"
```

Adjust the `database_*` values to match your PostgreSQL instance. The database
schema is created automatically on startup (refinery migrations from the
`migrations/` directory).

### Data sources

External data is imported from one or more **data sources**, each configured as
an array entry under `[[data_sources]]`:

- `name` – unique display name (used to derive the stable data-source id and to
  label the health component). UTF-8 names such as `Münster` are supported.
- `provider.type` – the provider implementation to build (e.g.
  `münster_opendata_github_provider`).
- `provider.vars` – provider-specific key/value settings. The supported keys
  depend on the provider only; the Münster provider understands `url` (required),
  `max_measurement_batch_size` (optional, defaults to `500`),
  `max_measurement_timeframe_hours` (optional hours, defaults to `168` — the
  import time window) and `cache_duration` (optional seconds, defaults to `300` —
  the archive-cache window).

On startup the application syncs the configured data sources into the
`data_sources` table: new ones are added, ones that are no longer configured are
removed. The `data_sources` list may be empty (no import happens, but the API
and health checks still work).

### Background data-source updates

A cron-driven scheduler keeps the configured data sources up to date. Every run
is recorded as a generic ShedLock-style **job** in the `jobs` table and goes
through a lifecycle: `PENDING -> RUNNING -> FINISHED` (or `FAILED`).

- `data_source_update_cron` – CRON expression that re-triggers the update job.
  Defaults to `"0 0 * * * *"` (every hour) and is validated at startup.
- `data_source_update_max_lifetime_seconds` – **required** (no default): the max
  lifetime of an update job. Each job gets an absolute `lifetime_until` deadline
  (`insert time + max lifetime`) stored as `TIMESTAMPTZ`. A `RUNNING` job blocks
  other runs of the same type **only until** that deadline; a stale `RUNNING` job
  past its deadline is expired to `FAILED` with `max_lifetime_exceeded = true`,
  so the next run can proceed even after a crash.

Scheduling semantics:

- The same always-on rule applies at startup and on every cron tick: the job runs
  if it has never succeeded or if the last successful run is overdue (the next
  scheduled trigger after its `finished_at` has already passed). A missed slot
  while the process was down is therefore caught up on the next startup.
- Afterwards it runs on the CRON schedule.
- While a job of the same type is `RUNNING` and within its `lifetime_until`, new
  runs are skipped (a warning is printed).
- Updates are **incremental**: each data source's `imported_until` advances to
  the last processed measurement timestamp, so consecutive runs do not reprocess
  data. Per data source the order is strict: counting stations, then channels,
  then measurements (paged in batches; the running `processed_measurements` and
  `added_measurements` counts are persisted to the job metadata after each
  batch). `DELETE /api/v1/data-sources/{id}/imported_until` clears the cursor to
  force a full re-import of one data source.

The Münster provider downloads the configured GitHub ZIP, extracts it into an
obscured `/tmp` folder, and serves counting stations, channels and measurements
from the extracted files. The archive is cached with the persistent-state handle
(`archive_downloaded_at`, `archive_extracted_at`, `archive_file`,
`archive_extracted_dir`, `archive_etag`, `archive_last_modified`): a fresh
extracted folder is reused, a fresh ZIP is re-extracted, and once the
`cache_duration` window passes the provider re-downloads (skipped when a
best-effort `HEAD` shows the upstream `ETag`/`Last-Modified` is unchanged). The
station/channel metadata comes from `site_min.json`; measurements come from the
per-station `YYYY-MM.csv` files (15-minute intervals, interpreted as
Europe/Berlin local time and stored as UTC). The station-aggregate column and the
`-status` columns are ignored. The channel→file map is derived from the station
directories (every channel of a station lives in that station's monthly files),
so building the index never reads CSV headers.

The measurements import is bounded by a **time window** so even the first
multi-year import stays responsive: each provider call only reads the monthly
files overlapping `(cursor, cursor + max_measurement_timeframe_hours]` (default
7 days), and the core keeps paging until every channel is fully imported. Rows
are written idempotently on the natural key `(channel_id, timestamp)`
(`INSERT ... ON CONFLICT DO NOTHING`), so a partially-completed run can always
be resumed without duplicating data.

Every job is exposed through the read-only jobs API (see below).

### Asset storage (counting-station images)

Counting-station images live in an **S3-compatible object store** (MinIO in the
docker-compose stack). PostgreSQL stores only the asset **metadata** and the
station↔asset link; the binary bytes never touch the database.

- `[asset_storage]` – the S3 connection settings: `endpoint`, `access_key`,
  `secret_key`, `bucket`, `region`. Under docker compose the bucket is created
  automatically by a one-shot `mc` container (`minio-init`) that runs before the
  backend starts.
- Images come from two places:
  - **data-source providers**, with hash-based change detection
    (`CountingStation.image_sha256`): the provider reports a hash with each
    station record and the core only fetches the image bytes when the hash
    changed or the station has no linked asset yet;
  - **built-in** images embedded in the backend binary. The list is derived by
    scanning `backend/assets/` at compile time (`include_dir`): every file in the
    folder becomes a `builtin/{path}` object with its content type inferred from
    the extension, so adding or removing a file is the only step needed to change
    what is synced. The emerald plain bike icon
    (`builtin/bike-icon-black-transparent.svg`) is the fallback every station
    without a provider image points to. The built-in sync is idempotent and
    reconciles **both directions** — it uploads/updates the bundled images and
    **removes** stale ones whose files were deleted from the folder, so the
    bucket and `assets` table mirror the assets folder. The frontend brand bike
    icon (favicon + header) lives separately in `frontend/public/bike-icon.svg`
    and is never streamed through the BFF; frontend brand assets belong in
    `frontend/public/`, backend builtin assets in `backend/assets/`. The Leaflet
    map marker (the emerald pin + bike, also never streamed) lives in the map
    feature as `frontend/src/features/map/map-flag-counting-station.svg` and is
    imported by `frontend/src/lib/leaflet.ts` via Vite, so editing that SVG file
    restyles both the map and the detail-preview markers without a code change.
- The **BFF streams** image bytes to the browser
  (`GET /api/bff/assets/{id}/content`); MinIO is reachable only from the backend
  (private `asset_network`) and never exposed to the browser.
- A dedicated **asset cleanup** scheduled job (`asset_cleanup`, default daily at
  04:00) deletes **orphaned objects** — object keys in the bucket that have no
  row in the `assets` table (left behind when a provider image hash changes or
  after a crash between `put` and `save`):
  - `asset_cleanup_cron` – validated CRON expression, defaults to
    `"0 0 4 * * *"`.
  - `asset_cleanup_max_lifetime_seconds` – **required** (no default), same
    ShedLock-style deadline semantics as the data-source update job.

### Persistent provider state

Each configured data source can remember opaque **runtime state** that survives
restarts (for example archive-cache metadata). The state lives in the
`data_source_persistent_state` table, scoped per data source:

- `data_source_id` – foreign key to `data_sources` (`ON DELETE CASCADE`); a data
  source has exactly one provider, so the id fully scopes the state.
- `key` / `value` – arbitrary opaque strings (`UNIQUE (data_source_id, key)`).
- `id` – surrogate UUID (application-generated) used purely for identification.

At startup every provider receives a scoped state handle **after** its data source
is persisted (two-phase handover): the provider is constructed first without the
handle, then `StartupService` calls `attach_persistent_state` to hand it the
handle. Neither the core nor REST interprets keys or values — only the provider
adapter does.

When a data source's `provider_type` changes, a database trigger
(`AFTER UPDATE OF provider_type`) deletes its state rows, so a different provider
never inherits the previous provider's memory; deleting a data source cascades to
its state.

The state is exposed through the core as
`GET/PUT/DELETE /api/v1/data-sources/{id}/persistent_state` (see
[API overview](#api-overview)): the driving adapter calls a core application
service (`PersistentStateService`), never a repository port directly. The
pre-existing read endpoints still call their repositories directly; migrating them
to core services is a separate follow-up.

### Data-provider messages

Each data source can also accumulate **provider messages** (events) that the
import records as it works. These are stored in the
`data_source_provider_messages` table, scoped per data source:

- `data_source_id` – foreign key to `data_sources` (`ON DELETE CASCADE`).
- `severity` – one of `INFO`, `WARNING`, `ERROR`, `DEBUG`, `TRACE`.
- `message` – human-readable event text.
- `occurred_at` – `TIMESTAMPTZ` auto-generated by the database on insert.
- `id` – surrogate UUID (application-generated) used purely for identification.

At startup every provider receives a scoped message sink **after** its data
source is persisted (the same two-phase handover as persistent state):
`StartupService` calls `attach_provider_messages`, handing the provider a
`ProviderMessageSink` whose `provider_event_occurred(severity, message)` writes a
row. Emitting a message is best-effort and never fails the import.

The Münster provider uses this to report non-fatal data quirks instead of
aborting: when a channel has no column in a monthly CSV file it records a
`WARNING` (with the channel id and file path) and skips that file, so the update
continues and finishes. Genuinely fatal conditions still abort.

Messages are exposed read-only through the core as
`GET /api/v1/data-sources/{id}/messages` (see [API overview](#api-overview)):
the driving adapter calls a core application service (`ProviderMessageService`),
never a repository port directly.

## Start the database

If you do not have a PostgreSQL instance yet, start one matching the defaults:

```bash
docker run --name bike_counter_db \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=bike_counter \
  -p 5432:5432 \
  -d postgres
```

## Run

The easiest way to run the whole stack (PostgreSQL + backend + frontend) is
through the [`Makefile`](Makefile):

```bash
make run    # docker compose up --build (foreground, follows logs; Ctrl-C to stop)
make down   # stop and remove the stack (keeps the database volume)
make logs   # follow the logs of all services
```

This requires a [`config.toml`](config.toml) with `database_url` set to the
compose service name `db` (see [Run with Docker Compose](#run-with-docker-compose)).

Alternatively, run the backend binary locally (reads `config.toml` from the
working directory, applies migrations, serves the API) — this needs a reachable
PostgreSQL, so set `database_url="postgres://localhost:5432"`:

```bash
cd backend
cargo build
cargo run
```

The backend prints the loaded configuration and then serves:

| Resource               | URL                                        |
|------------------------|--------------------------------------------|
| Frontend (React)       | <http://localhost:8081>                    |
| BFF API base           | <http://localhost:8080/api/bff>            |
| REST API base          | <http://localhost:8080/api/v1>             |
| Swagger-UI             | <http://localhost:8080/swagger-ui/>        |
| OpenAPI JSON document  | <http://localhost:8080/api-docs/openapi.json> |
| Liveness               | <http://localhost:8080/health/live>        |
| Readiness              | <http://localhost:8080/health/ready>       |

## Run with Docker Compose

The repository includes Dockerfiles for both sub-projects and a
[`docker-compose.yml`](docker-compose.yml) that ramps up the whole stack:

```bash
# Copy the template, adjust database_url to "postgres://db:5432", then start
cp config.toml.example config.toml
# Either directly, or via the Makefile:
docker compose up --build        # make run
docker compose down              # make down
docker compose logs -f           # make logs
```

- The `db` service runs PostgreSQL with the development defaults
  (`postgres` / `postgres` / `bike_counter`) and persists data in a named volume.
- The `minio` service runs a private S3-compatible object store for counting-
  station images on an **internal-only** `asset_network` (no host port) with its
  own named volume; the one-shot `minio-init` service (MinIO client `mc`) creates
  the image bucket before the backend starts. Only the backend is attached to
  `asset_network`, so MinIO is never reachable from `db`, `frontend` or outside.
- The `backend` service builds the Rust binary inside a multi-stage Docker build
  and mounts [`config.toml`](config.toml) into the container. Its
  [`backend/docker/entrypoint.sh`](backend/docker/entrypoint.sh) refuses to start
  without a `config.toml` and otherwise just runs the REST server (which applies
  the refinery migrations on startup).
- The `frontend` service builds the React app (Vite) into static assets served by
  nginx, which reverse-proxies `/api` to the `backend` service so the browser
  only ever talks same-origin (no CORS). It is exposed on <http://localhost:8081>.
- **All configuration is TOML-only.** There are no configuration environment
  variables and no `.env` file – neither for the database nor for data sources.
  When running under docker compose, set `database_url` to the compose service
  name `db`:

  ```toml
  database_url="postgres://db:5432"
  database_user="postgres"
  database_password="postgres"
  database_name="bike_counter"

  data_source_update_cron="0 0 * * * *"
  data_source_update_max_lifetime_seconds=3600

  asset_cleanup_cron="0 0 4 * * *"
  asset_cleanup_max_lifetime_seconds=3600

  [asset_storage]
  endpoint = "http://minio:9000"
  access_key = "minioadmin"
  secret_key = "minioadmin"
  bucket = "bike-counter-images"
  region = "us-east-1"

  [[data_sources]]
  name = "Münster"

  [data_sources.provider]
  type = "münster_opendata_github_provider"

  [data_sources.provider.vars]
  url = "https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"
  max_measurement_batch_size = "500"
  ```

  The mounted `config.toml` is used as-is and can contain both the database
  settings and the `[[data_sources]]` sections.

Useful commands:

```bash
docker compose logs -f app   # follow application logs
docker compose down          # stop containers (keep the database volume)
docker compose down -v       # stop containers and delete the database volume
```

## API overview

The API uses a flat URL hierarchy under `/api/v1`. The resource endpoints are
**read-only (GET)**; the `persistent_state` endpoints additionally support `PUT`
and `DELETE` to manage the opaque per-data-source provider state, and the
counting-station endpoint supports `PATCH` to set a station's GPS coordinates:

- `GET /api/v1` – root discovery with HATEOAS links
- `GET /api/v1/data-sources` / `GET /api/v1/data-sources/{id}` – list / fetch the configured (persisted) data sources
- `GET /api/v1/data-sources/{id}/persistent_state` – full opaque persistent-state map for a data source
- `PUT /api/v1/data-sources/{id}/persistent_state/{key}` with body `{"value": "..."}` – upsert one entry (`200`; `404` unknown data source; `400` blank key)
- `DELETE /api/v1/data-sources/{id}/persistent_state/{key}` – delete one entry (`204`; `404` unknown data source)
- `DELETE /api/v1/data-sources/{id}/persistent_state` – clear the whole store (`204`; `404` unknown data source)
- `GET /api/v1/data-sources/{id}/messages` – read-only provider messages for a data source, newest first
- `GET /api/v1/jobs` (optional `?job_type=` and `?status=` filters) / `GET /api/v1/jobs/{id}` – list / fetch the tracked background jobs
- `GET /api/v1/counting-stations` (optional `?name=` substring filter) / `GET /api/v1/counting-stations/{id}` — stations carry optional `latitude`/`longitude` (WGS84)
- `PATCH /api/v1/counting-stations/{id}` with body `{"latitude": ..., "longitude": ...}` (fields optional; `null` clears a coordinate) — sets a station's GPS coordinates
- `GET /api/v1/channels` (optional `?counting_station_id=` and `?name=` substring filters) / `GET /api/v1/channels/{id}`
- `GET /api/v1/measurements` (optional `?channel_id=` filter plus `?offset=`/`?limit=` pagination, newest first; `limit` defaults to 5000 with no upper bound) / `GET /api/v1/measurements/{id}`
- `GET /api/v1/measurements/raw` – lean bulk export: same `?channel_id=`, `?offset=`/`?limit=` parameters, but returns a bare JSON array of plain measurement objects (no HATEOAS links and no pagination envelope) for scraping large volumes

Every resource includes a `_links` object (HAL-style) pointing to related
resources, e.g. a station links to its own `self`, its `channels`, its
`collection`, and its `data_source`; counting stations also expose the
`data_source_id` field of the data source they were imported from. A data
source links to its `self`, `collection`, `persistent_state`, and
`messages`, plus an RFC 6570 templated `persistent_state_entry` for a single key
(marked `"templated": true`). The root discovery endpoint (`/api/v1`)
additionally links to the operational health endpoints via `health-live` and
`health-ready`.

### BFF API (frontend-only)

In addition to the public `/api/v1` REST API, the backend exposes a
**Backend-for-Frontend** API reserved for the React frontend. It lives under
`/api/bff` and is documented in the **same** Swagger document but grouped under
its own `BFF API` collection/tag so the frontend-facing calls are easy to spot:

- `GET /api/bff/stations` – map markers for the current viewport. Requires the
  `min_lat`/`min_lng`/`max_lat`/`max_lng` bounding-box query and returns only the
  **positioned** stations inside it. Each item carries only what the map needs:
  `id`, `name`, `latitude`, `longitude`.
- `GET /api/bff/stations/sidebar` – station summaries for the current viewport
  (same required bounding box). Each item carries `id`, `name`, `description`,
  `latitude`, `longitude`, `channel_count` and `bikes_last_day` (the sum of
  `measurements.value` across the station's channels on the **previous complete
  local day**, computed on the fly in the station's own timezone, DST-aware),
  plus `visible_count` / `total_count` (stations visible in the viewport vs. all
  counting stations).
- `GET /api/bff/stations/search` – every counting-station summary (no bounds)
  plus the map of possible actions (for now `find_on_map` is always enabled).
- `GET /api/bff/global-summary` – whole-system statistics for the header:
  `station_count`, `channel_count`, `bikes_last_day_total` (sum of every
  station's previous local-day total) and the `last_update` timestamp of the most
  recent successful data-source update.
- `GET /api/bff/station-overview/{id}` – the **page-shaped** overview payload for
  one station: `id`, `name`, `description`, `latitude`, `longitude`,
  `channel_count`, `image_url`, `last_update`, `detail_url` (`/stations/{id}`)
  and a `metrics` array — each with `key` (`last_day` / `last_7_days` /
  `last_month` / `last_year`), `current`, `previous`, `trend`
  (`up`/`down`/`flat`) and `delta_percent`. The metrics use **complete calendar
  periods** in the station's own timezone (previous full local day, previous 7
  full local days, previous full calendar month, previous full calendar year),
  each compared with the immediately preceding equal-length period. The payload
  is flat — no HATEOAS `_links`, no `data_source_id`, no REST `CountingStationDto`
  reuse.
- `GET /api/bff/station-detail/{id}` – the **page-shaped** detail payload for the
  detail page: the station metadata + `metrics` from `station-overview/{id}`
  (incl. `last_year`), a `channels` array (id + name for legends/pie labels) and
  a `graphs` object keyed by the four selectable timeframes — `day` (last day vs
  the day before, 5 min), `week` (current vs last week, 1 h), `last_30_days`
  (last 30 days vs the 30 days before, 1 day) and `year` (current vs last year,
  1 day). Each timeframe holds its `current` / `previous` series, a
  `weekday_radar` and `channel_pie` for its current period and a `per_channel`
  copy for the nerd stats; `monthly_totals` feeds the standalone monthly bar
  chart. Buckets are aligned to the station's own timezone via PostgreSQL
  `date_bin` and are **data-only** (no zero-filling, so a running week/year
  simply ends at the latest measurement).
- `GET /api/bff/stations/summary` – the **page-shaped** aggregated summary of the
  stations visible in the bounding box (all four bounds required; optional
  `exclude=<comma-separated station ids>` drops stations from the aggregation
  while keeping them in the returned `stations` list so the map can gray them
  out). The payload mirrors the detail page shape: a fallback `image_url`, the
  `stations` (id/name/lat/lng/channel_count), the aggregated `channel_count`, the
  four aggregated overview `metrics` (each station's DST-aware windows) and the
  `graphs` (same four timeframes + `monthly_totals`) whose nerd stats are keyed by
  **station** (`per_station`, `station_pie`) instead of channel. All bucketed
  reads reuse the existing `MeasurementRepository` primitives over the union of
  the included stations' channels, so no new data fields are introduced.
- `GET /api/bff/assets/{id}/content` – streams an asset (e.g. the station image)
  from MinIO with `Content-Type`, `ETag`, `Content-Length` and a `Cache-Control`
  (`immutable` for built-in assets, short-lived for provider assets). Only the
  BFF exposes MinIO; there are no upload/delete artifact endpoints.

The aggregations are computed **on the fly** per request by the core
`StationSummaryService` / `GlobalSummaryService` / `StationsSummaryService`; a
cache (e.g. Redis) may be introduced later. The BFF module lives in
[`backend/src/adapter/driving/bff/`](backend/src/adapter/driving/bff) and is the
seam for future frontend-only endpoints (for example aggregations or
transformations of the `/api/v1` data).

## Name uniqueness

Two naming invariants are enforced on imported data (see
[`plans/19_counting_station_channel_name_uniqueness_plan.md`](plans/19_counting_station_channel_name_uniqueness_plan.md)):

- A counting-station name is unique **per data source**
  (`UNIQUE (data_source_id, name)` where `data_source_id IS NOT NULL`).
- A counting station has no two channels with the same name
  (`UNIQUE (counting_station_id, name)`).

When an upstream source does not provide unique names (true for the Münster
archive, which repeats channel names within a station), the adapter appends the
channel's/station's external id to the duplicate name, e.g.
`Bohlweg Fahrräder Stadteinwärts (353484923)`. Migration `V8` repairs rows that
were imported before this rule and creates the two unique indexes.

Counting stations always carry their importing `data_source_id`, enforced by
the database: `counting_stations.data_source_id` is `NOT NULL`. Rows imported
before data-source linking existed are backfilled by migration `V9` (only when
exactly one data source is configured); `V9` also switches the FK chain to
`ON DELETE CASCADE` so removing a data source removes its stations (and their
channels/measurements) instead of orphaning them.

## Health checks

The application exposes two unversioned operational endpoints:

- `GET /health/live` – liveness probe. Answers `200 {"status":"up"}` while the
  backend process is running (it requires no database access).
- `GET /health/ready` – readiness probe. Opens a fresh PostgreSQL connection and
  runs `SELECT 1`, and checks the health of every configured data-source
  provider. It answers `200 {"status":"ready", ...}` only when every downstream
  service is available, otherwise `503 {"status":"not_ready", ...}` with a
  per-component breakdown (including the failure reason).

Each data source contributes a component named
`<data-source-name>/<provider-type>` (e.g. `Münster/münster_opendata_github_provider`).
A provider that is temporarily unreachable marks the component as `down` but
does **not** block startup: the process only refuses to start on configuration
errors.

Both the `Dockerfile` `HEALTHCHECK` and the docker-compose `app` service use
`/health/ready`, so a container is only marked *healthy* while PostgreSQL is
reachable.

## Running tests

A [`Makefile`](Makefile) wraps the common commands. Run `make help` for the full
list:

```bash
make check      # backend CI gate: cargo fmt --check + cargo clippy --all-targets -- -D warnings
make test       # backend: all tests (repository tests spin up a Postgres test container via Docker)
make test-rest  # backend: only the REST endpoint tests (in-memory mocks, no database required)
make test-e2e   # end-to-end smoke test against the real docker-compose stack (requires Docker)
make test-playwright # frontend Playwright browser e2e tests against the real stack with a real Münster import (requires Docker + GitHub)
make playwright-install # install the Playwright Chromium browser once
make test-all   # make check + make test
make coverage   # backend coverage gate: overall (production) >= 80% AND core (src/core) >= 95% via cargo-llvm-cov
make coverage-open  # open the HTML coverage report in a browser
make frontend-build # build the React frontend (production bundle into frontend/dist)
```

The Cargo-based targets operate on the [`backend/`](backend) crate; the
frontend is built with `make frontend-build` (or `cd frontend && npm run build`).

Under the hood the scripts are:

- [`scripts/fmt-test.sh`](scripts/fmt-test.sh) – CI-style gate: `cargo fmt --check`
  and `cargo clippy --all-targets -- -D warnings`, failing non-zero on any drift.
- [`scripts/docker-compose-test.sh`](scripts/docker-compose-test.sh) – boots the
  real docker-compose stack (PostgreSQL + app), waits for readiness, asserts the
  jobs + data-sources APIs return `200`, verifies a `data_source_update` job with
  `lifetime_until` was recorded, and checks `jobs.lifetime_until TIMESTAMPTZ NOT
  NULL` and `data_sources.imported_until` via `psql`, then tears everything down.
- [`scripts/e2e-playwright.sh`](scripts/e2e-playwright.sh) – Playwright browser
  e2e tests against the real stack. Boots PostgreSQL + backend + frontend with a
  real `Münster` data source (imported from GitHub at startup), waits for
  readiness and the counting-station import, then runs the specs in
  [`frontend/e2e/`](frontend/e2e): clicking a map marker opens its popup, the
  search dialog finds a station and "Find on map" opens the matching popup, and
  the sidebar renders only the stations visible in the current viewport. Tears
  the stack down afterwards and restores any pre-existing `config.toml`.
- [`scripts/coverage.sh`](scripts/coverage.sh) – coverage gate: runs the full
  test suite under `cargo-llvm-cov` instrumentation, writes the standard lcov +
  HTML report under `target/coverage/`, and fails non-zero when **overall
  production** line coverage drops below `COVERAGE_THRESHOLD` (default 80%) or
  when the **core** (`src/core/`) drops below `CORE_COVERAGE_THRESHOLD` (default
  95%). Both thresholds count production code only — `#[cfg(test)]` scaffolding
  and standalone test files are excluded — so the gate's percentages are printed
  to the terminal and differ from the totals in the standard HTML report (which
  still includes test scaffolding). The core is expected to be unit-tested in
  isolation with in-memory mocks, hence the higher bar.

`fmt-test.sh` and `coverage.sh` are intended to be wired into CI;
`docker-compose-test.sh` requires Docker and `docker compose` v2. Install the
coverage tooling once (`rustup component add llvm-tools-preview` and
`cargo install cargo-llvm-cov`); the full coverage run needs Docker for the
Postgres repository tests, like `make test`.
