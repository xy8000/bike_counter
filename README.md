# Bike-Counter

This Repository can be used to analyse the Bike-Counter-Stations of Münster.

It exposes a read-only REST API (with HATEOAS links) backed by a PostgreSQL
database, documented via auto-generated OpenAPI and browsable through Swagger-UI.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain, edition 2024)
- A running PostgreSQL server (see [Configuration](#configuration) below)

## Configuration

All settings live in [`config.toml`](config.toml):

```toml
github_data_url="https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"
database_url="postgres://localhost:5432"
database_user="postgres"
database_password="postgres"
database_name="bike_counter"
```

Adjust the `database_*` values to match your PostgreSQL instance. The database
schema is created automatically on startup (refinery migrations from the
`migrations/` directory).

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

The repository includes a [`Dockerfile`](Dockerfile), a [`docker-compose.yml`](docker-compose.yml)
and a `.env` file that ramp up both the PostgreSQL database and the application:

```bash
# Copy the defaults and adjust credentials/ports if needed
cp .env .env.local   # or edit .env directly

# Build and start everything (db + app)
docker compose up --build
```

- The `db` service runs PostgreSQL with the credentials from `.env` and persists data in a named volume.
- The `app` service builds the Rust binary inside a multi-stage Docker build. On startup its
  [`docker/entrypoint.sh`](docker/entrypoint.sh) renders `config.toml` from the environment
  variables in `.env` (the database host is `db`, the compose service name), runs the refinery
  migrations and starts the REST server.

Useful commands:

```bash
docker compose logs -f app   # follow application logs
docker compose down          # stop containers (keep the database volume)
docker compose down -v       # stop containers and delete the database volume
```

> Note: `.env` is gitignored. The checked-in defaults live in [`docker-compose.yml`](docker-compose.yml)
> and the README; copy/adapt `.env` to your needs and never commit real secrets.

## API overview

All endpoints are **read-only (GET)** and use a flat URL hierarchy under `/api/v1`:

- `GET /api/v1` – root discovery with HATEOAS links
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
  runs `SELECT 1`. It answers `200 {"status":"ready", ...}` only when every
  downstream service is available, otherwise `503 {"status":"not_ready", ...}`
  with a per-component breakdown (including the failure reason).

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
