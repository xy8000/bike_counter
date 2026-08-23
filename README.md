# Bike-Counter

This Repository can be used to analyse the Bike-Counter-Stations of Münster.

It exposes a REST API (with HATEOAS links) backed by a PostgreSQL database,
documented via auto-generated OpenAPI and browsable through Swagger-UI.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain, edition 2024)
- A running PostgreSQL server (see [Configuration](#configuration) below)

## Configuration

All settings live in [`config.toml`](config.toml) (gitignored). Start from the
tracked template [`config.toml.example`](config.toml.example):

```bash
cp config.toml.example config.toml
```

```toml
database_url="postgres://localhost:5432"
database_user="postgres"
database_password="postgres"
database_name="bike_counter"

# CRON expression for the data-source update job (default: every hour).
data_source_update_cron="0 0 * * * *"
# REQUIRED ShedLock-style max lifetime for the update job in seconds (no default).
data_source_update_max_lifetime_seconds=3600

[[data_sources]]
name = "Münster"

[data_sources.provider]
type = "münster_opendata_github_provider"

[data_sources.provider.vars]
url = "https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"
max_measurement_batch_size = "500"
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
  `max_measurement_batch_size` (optional, defaults to `500`) and `cache_duration`
  (optional seconds, defaults to `300` — the archive-cache window).

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
- Updates are **incremental**: each data source's `last_updated_at` advances to
  the last processed measurement timestamp, so consecutive runs do not reprocess
  data. Per data source the order is strict: counting stations, then channels,
  then measurements (paged in batches; the running `processed_measurements`
  count is persisted to the job metadata after each batch).

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
`-status` columns are ignored.

Every job is exposed through the read-only jobs API (see below).

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

The easiest way to run the whole stack (PostgreSQL + application) is through the
[`Makefile`](Makefile):

```bash
make run    # docker compose up --build (foreground, follows logs; Ctrl-C to stop)
make down   # stop and remove the stack (keeps the database volume)
make logs   # follow the application logs
```

This requires a [`config.toml`](config.toml) with `database_url` set to the
compose service name `db` (see [Run with Docker Compose](#run-with-docker-compose)).

Alternatively, run the binary locally (reads `config.toml`, applies migrations,
serves the API) — this needs a reachable PostgreSQL, so set
`database_url="postgres://localhost:5432"`:

```bash
cargo build
cargo run
```

The application prints the loaded configuration and then serves:

| Resource               | URL                                        |
|------------------------|--------------------------------------------|
| REST API base          | <http://localhost:8080/api/v1>             |
| Swagger-UI             | <http://localhost:8080/swagger-ui/>        |
| OpenAPI JSON document  | <http://localhost:8080/api-docs/openapi.json> |
| Liveness               | <http://localhost:8080/health/live>        |
| Readiness              | <http://localhost:8080/health/ready>       |

## Run with Docker Compose

The repository includes a [`Dockerfile`](Dockerfile) and a [`docker-compose.yml`](docker-compose.yml)
that ramp up both the PostgreSQL database and the application:

```bash
# Copy the template, adjust database_url to "postgres://db:5432", then start
cp config.toml.example config.toml
# Either directly, or via the Makefile:
docker compose up --build        # make run
docker compose down              # make down
docker compose logs -f app       # make logs
```

- The `db` service runs PostgreSQL with the development defaults
  (`postgres` / `postgres` / `bike_counter`) and persists data in a named volume.
- The `app` service builds the Rust binary inside a multi-stage Docker build and
  mounts a [`config.toml`](config.toml) into the container. Its
  [`docker/entrypoint.sh`](docker/entrypoint.sh) refuses to start without a
  `config.toml` and otherwise just runs the REST server (which applies the
  refinery migrations on startup).
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
and `DELETE` to manage the opaque per-data-source provider state:

- `GET /api/v1` – root discovery with HATEOAS links
- `GET /api/v1/data-sources` / `GET /api/v1/data-sources/{id}` – list / fetch the configured (persisted) data sources
- `GET /api/v1/data-sources/{id}/persistent_state` – full opaque persistent-state map for a data source
- `PUT /api/v1/data-sources/{id}/persistent_state/{key}` with body `{"value": "..."}` – upsert one entry (`200`; `404` unknown data source; `400` blank key)
- `DELETE /api/v1/data-sources/{id}/persistent_state/{key}` – delete one entry (`204`; `404` unknown data source)
- `DELETE /api/v1/data-sources/{id}/persistent_state` – clear the whole store (`204`; `404` unknown data source)
- `GET /api/v1/jobs` (optional `?job_type=` and `?status=` filters) / `GET /api/v1/jobs/{id}` – list / fetch the tracked background jobs
- `GET /api/v1/counting-stations` / `GET /api/v1/counting-stations/{id}`
- `GET /api/v1/channels` (optional `?counting_station_id=` filter) / `GET /api/v1/channels/{id}`
- `GET /api/v1/measurements` (optional `?channel_id=` filter) / `GET /api/v1/measurements/{id}`

Every resource includes a `_links` object (HAL-style) pointing to related
resources, e.g. a station links to its own `self`, its `channels`, and its
`collection`; a data source links to its `self`, `collection`, and
`persistent_state`, plus an RFC 6570 templated `persistent_state_entry` for a
single key (marked `"templated": true`). The root discovery endpoint
(`/api/v1`) additionally links to the operational health endpoints via
`health-live` and `health-ready`.

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
make check      # CI gate: cargo fmt --check + cargo clippy --all-targets -- -D warnings
make test       # all tests (repository tests spin up a Postgres test container via Docker)
make test-rest  # only the REST endpoint tests (in-memory mocks, no database required)
make test-e2e   # end-to-end smoke test against the real docker-compose stack (requires Docker)
make test-all   # make check + make test
```

Under the hood the scripts are:

- [`scripts/fmt-test.sh`](scripts/fmt-test.sh) – CI-style gate: `cargo fmt --check`
  and `cargo clippy --all-targets -- -D warnings`, failing non-zero on any drift.
- [`scripts/docker-compose-test.sh`](scripts/docker-compose-test.sh) – boots the
  real docker-compose stack (PostgreSQL + app), waits for readiness, asserts the
  jobs + data-sources APIs return `200`, verifies a `data_source_update` job with
  `lifetime_until` was recorded, and checks `jobs.lifetime_until TIMESTAMPTZ NOT
  NULL` and `data_sources.last_updated_at` via `psql`, then tears everything down.

`fmt-test.sh` is intended to be wired into CI; `docker-compose-test.sh` requires
Docker and `docker compose` v2.
