# Bike-Counter

[![GitHub Release](https://img.shields.io/github/v/release/xy8000/bike_counter?sort=semver)](https://github.com/xy8000/bike_counter/releases)
[![Backend pulls](https://img.shields.io/docker/pulls/xy8000/bike-counter-backend?label=backend%20pulls)](https://hub.docker.com/r/xy8000/bike-counter-backend)
[![Frontend pulls](https://img.shields.io/docker/pulls/xy8000/bike-counter-frontend?label=frontend%20pulls)](https://hub.docker.com/r/xy8000/bike-counter-frontend)

**Bike-Counter** lets you explore and analyse the public bicycle-counting
stations of **Münster, Bonn, Hamburg, Leipzig** and selected **Eco-Counter**
stations. The measurement data is imported automatically from the cities'
official Open Data / public sources — most cities need no API keys.

The project ships as a self-hosted web application plus an HTTP API:

- a **React frontend** with an interactive map, station statistics and charts;
- a **Rust backend** (Axum) backed by **PostgreSQL** that imports, stores and
  aggregates the data;
- a **Docker Compose** stack that runs the whole system with one command.

## Features

- **Interactive map** — one marker per counting station that has GPS
  coordinates, on a self-hosted basemap (no third-party tile servers). Stations
  that sit close together are grouped into clusters that zoom in when clicked.
- **Search** — find a station by name and jump straight to it on the map.
- **Sidebar** — lists the stations currently visible on the map, each with its
  name, description, channel count and the bikes counted in the latest full
  local day.
- **Live summary** — the header shows how many stations and channels are
  tracked and how many bikes were counted across all of them, together with the
  last successful data update.
- **Station detail page** (`/stations/:id`) — per station: overview statistics
  (bikes in the last day / week / month / year and the all-time total), its
  image, and a shared timeframe selector (last 24 hours, current + previous
  week, last 30 days, the current year, or an individual date range). The
  selector drives bar charts that compare the selected period with the previous
  one, weekday and hour-of-day radars, the share per channel, detailed
  per-channel statistics and a "bikes per month" chart.
- **Station summary page** (`/summary`) — aggregate the stations currently
  visible on the map into one comparison view (charts + per-station
  statistics). The view is encoded in the URL, so it can be shared and
  restored.
- **Trend settings** — an "exclude new stations from trends" option keeps
  stations that only started counting recently from skewing comparisons.
- **Dark mode & responsive layout** — the UI follows the operating system's
  colour scheme and adapts from phones (full-screen drawer) to tablets and
  desktops.
- **Machine access** — a public REST API and an OpenData bulk export for
  automated consumers (see [Open data & API](#open-data--api)).

## Quick start

Run the whole stack (PostgreSQL + backend + frontend) with Docker and the
Compose v2 plugin:

```bash
# 1. Create your configuration from the tracked template
cp config.toml.example config.toml

# 2. Start the stack (via the Makefile, or directly: docker compose up --build)
make run

# 3. Open the web app
open http://localhost:8081
```

The first start builds the Docker images and generates the self-hosted map
basemap (this downloads the pinned map extract, so it needs internet access and
takes a few minutes). `make down` stops the stack and keeps the database;
`make logs` follows the logs of all services.

> The template enables every supported city. If you only want a subset — or no
> automatic imports at all — edit the `[[data_sources]]` entries in
> [`config.toml`](config.toml); see [Configuration](#configuration).

### Running the released images

Instead of building from source, the released images on Docker Hub can be
pulled directly — the `backend` and `frontend` services in
[`docker-compose.yml`](docker-compose.yml) pin the published image names
(`xy8000/bike-counter-backend:0.0.1`, `xy8000/bike-counter-frontend:0.0.1`)
alongside their `build:` blocks, so `docker compose pull` fetches the release
and `docker compose up` (without `--build`) runs it:

```bash
# 1. Create your configuration from the tracked template
cp config.toml.example config.toml

# 2. Pull the released images
docker compose pull

# 3. Start the stack
docker compose up -d
```

The first start downloads and builds the self-hosted map basemap (it needs
internet access and takes a few minutes); `make down` stops the stack and keeps
the database. New releases ship as GitHub releases with matching Docker Hub
tags (`0.0.1`, `latest`).

## Services

Once running, the stack exposes:

| What | URL |
| --- | --- |
| Web application | <http://localhost:8081> |
| Swagger-UI (API documentation) | <http://localhost:8080/swagger-ui/> |
| OpenAPI JSON document | <http://localhost:8080/api-docs/openapi.json> |
| Public REST API | <http://localhost:8080/api/v1> |
| BFF API (used by the web app only) | <http://localhost:8080/api/bff> |

Operational health endpoints: `GET /health/live` (liveness) and
`GET /health/ready` (readiness).

## Configuration

All configuration lives in a single [`config.toml`](config.toml) file
(gitignored), and every option is documented line by line in the tracked
template [`config.toml.example`](config.toml.example). The database schema is
created and migrated automatically on startup; scheduled background jobs keep
the imported data up to date.

- **Database** — `database_url`, `database_user`, `database_password`,
  `database_name`. Under Docker Compose point them at the `db` service
  (`postgres://db:5432`, as in the template).
- **Data sources** — one `[[data_sources]]` entry per source, with a display
  `name`, a provider `type` and provider-specific `vars`. Currently supported:

  | Source | Provider type |
  | --- | --- |
  | Münster (public Open Data archive) | `münster_opendata_github_provider` |
  | Bonn (official GeoJSON + CSV) | `bonn_opendata_http_provider` |
  | Hamburg (official SensorThings API) | `hamburg_sta_http_provider` |
  | Leipzig (official WFS layers) | `leipzig_wfs_http_provider` |
  | Eco-Counter (public Eco-Visio API) | `eco_counter_v1_http_provider` |
  | Eco-Counter (official API, access token) | `eco_counter_v2_http_provider` |
  | Eco-Counter (public dashboard scrape) | `eco_counter_web_http_provider` |

  The exact `vars` for every provider are documented in the template and in the
  adapter READMEs under
  [`backend/src/adapter/driven/`](backend/src/adapter/driven).
- **Optional services** — `[asset_storage]` (S3/MinIO bucket holding the
  counting-station images), `[opendata]`/`[opendata_storage]` (bulk-export
  schedule and bucket) and `[maps]` (self-hosted basemap settings).

## Open data & API

Machine consumers get two HTTP surfaces plus a bulk export:

- **Public REST API** (`/api/v1`) — read-only resources for counting stations,
  channels, measurements, data sources and background jobs, with HATEOAS links.
- **OpenData bulk export** (`/api/v1/opendata`) — the processed, immutable
  measurement data as per-station and global **daily** and **monthly** files in
  `parquet`, `csv.gz` and `json`. Files are published once per period and never
  rewritten.
- **BFF API** (`/api/bff`) — the aggregation API used by the web frontend.

The complete endpoint reference (parameters, schemas, examples) is served by
Swagger-UI at <http://localhost:8080/swagger-ui/>.

## Contributing

Contributions are welcome. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the
repository layout, how to implement a new data-source adapter, and the workflow
+ gates every change must pass ([`agents.md`](agents.md)). Work is planned and
tracked in the numbered documents under [`plans/`](plans).

## License

Bike-Counter is licensed under the [Apache License 2.0](LICENSE).
