# Bike-Counter

This Repository can be used to analyse the Bike-Counter-Stations of Münster.

It exposes a read-only REST API (with HATEOAS links) backed by a PostgreSQL
database, documented via auto-generated OpenAPI and browsable through Swagger-UI.

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
  depend on the provider only; the Münster provider understands `url` (required)
  and `max_measurement_batch_size` (optional, defaults to `500`).

On startup the application syncs the configured data sources into the
`data_sources` table: new ones are added, ones that are no longer configured are
removed. The `data_sources` list may be empty (no import happens, but the API
and health checks still work).

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

## Build and run

```bash
# Build the project
cargo build

# Start the application (reads config.toml, runs migrations, serves the API)
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
docker compose up --build
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

All endpoints are **read-only (GET)** and use a flat URL hierarchy under `/api/v1`:

- `GET /api/v1` – root discovery with HATEOAS links
- `GET /api/v1/data-sources` / `GET /api/v1/data-sources/{id}` – list / fetch the configured (persisted) data sources
- `GET /api/v1/counting-stations` / `GET /api/v1/counting-stations/{id}`
- `GET /api/v1/channels` (optional `?counting_station_id=` filter) / `GET /api/v1/channels/{id}`
- `GET /api/v1/measurements` (optional `?channel_id=` filter) / `GET /api/v1/measurements/{id}`

Every resource includes a `_links` object (HAL-style) pointing to related
resources, e.g. a station links to its own `self`, its `channels`, and its
`collection`. The root discovery endpoint (`/api/v1`) additionally links to the
operational health endpoints via `health-live` and `health-ready`.

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

```bash
# REST endpoint tests (in-memory mocks, no database required)
cargo test adapter::driving::rest::tests

# All tests (repository tests spin up a Postgres test container via Docker)
cargo test
```
