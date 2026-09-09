# Contributing

Thanks for contributing to Bike-Counter. This file covers the conventions every
change must follow and — most importantly — **how to implement a new data
provider (adapter)**.

For the required workflow and the full gate list, read [`agents.md`](agents.md)
first. Everything in this file is on top of that.

## Repository layout

- [`backend/`](backend) — Rust (Axum) in a **hexagonal architecture**:
  - `src/core/domain` — the business model and **ports** (traits). The core
    never references anything in `src/adapter/`.
  - `src/core/application` — use-case services implementing the driving ports.
  - `src/adapter/driving` — inbound adapters (REST/BFF API, job scheduler).
  - `src/adapter/driven` — outbound adapters (Postgres repositories, config
    reader, MinIO asset storage, the data-provider adapters).
- [`frontend/`](frontend) — React (Vite) single-page app served by nginx.
- [`plans/`](plans) — numbered plan documents (see [`agents.md`](agents.md)).
- [`scripts/`](scripts) + [`Makefile`](Makefile) — the quiet build/gate tooling.

A **data source** is one configured external source; each data source has exactly
one **provider**. A provider is a driven adapter that implements the
[`DataProvider`](backend/src/core/domain/data_source/provider_port.rs:151) port
and is built by [`DataProviderFactoryImpl`](backend/src/adapter/driven/data_provider_factory.rs:12).
The Münster provider ([`muenster_github/`](backend/src/adapter/driven/muenster_github/adapter.rs:1))
is the reference implementation.

## Local development

Prerequisites:

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain, edition
  2024)
- [Node.js](https://nodejs.org) 24 LTS (for the React frontend; pinned in
  [`frontend/.nvmrc`](frontend/.nvmrc) — Vite 7 needs at least Node 20.19/22.12)
- [Docker](https://www.docker.com) with the Compose v2 plugin (required by the
  gate targets and the PostgreSQL test container)
- A reachable PostgreSQL instance — for a local run, start one with the
  development defaults:

  ```bash
  docker run --name bike_counter_db \
    -e POSTGRES_USER=postgres \
    -e POSTGRES_PASSWORD=postgres \
    -e POSTGRES_DB=bike_counter \
    -p 5432:5432 \
    -d postgres
  ```

Setup:

```bash
cp config.toml.example config.toml
```

Then run the backend binary locally — it reads `config.toml` from the working
directory and applies the migrations on startup. Point `database_url` at your
local instance (`postgres://localhost:5432`) and set `TILES_DIR` to a writable
directory so the backend can build the basemap:

```bash
cd backend
TILES_DIR=./tiles cargo run
```

The basemap is mandatory: the first run downloads the pinned `go-pmtiles` CLI
and the Protomaps extract, so it needs network access and takes a few minutes.
The whole stack (PostgreSQL + backend + frontend) can instead be started with
`make run`; see the root [`README.md`](README.md) for the user-facing quick
start. The Cargo-based gate targets operate on the [`backend/`](backend) crate,
and the frontend is built with `make frontend-build` (or
`cd frontend && npm run build`) after `npm ci` in [`frontend/`](frontend).

## Implementing a new Adapter (DataProvider)

### 1. Implement the `DataProvider` trait

Create `backend/src/adapter/driven/<provider>/` and implement
[`DataProvider`](backend/src/core/domain/data_source/provider_port.rs:151).
The trait is synchronous (it runs inside `spawn_blocking`) and `Send + Sync`:

| Method | Purpose | Notes |
|---|---|---|
| `check_health()` | report upstream reachability/auth | return [`HealthStatus`](backend/src/core/domain/health/indicator.rs:8) |
| `get_all_counting_stations()` | all stations from the source | returns [`CountingStationRecord`](backend/src/core/domain/data_source/provider_port.rs:88) |
| `get_all_channels()` | all channels from the source | returns [`ChannelRecord`](backend/src/core/domain/data_source/provider_port.rs:117) |
| `get_measurements(query)` | one page of measurements | honors `query.max_batch_size`; returns [`MeasurementBatch`](backend/src/core/domain/data_source/provider_port.rs:135) |
| `max_measurement_batch_size()` | default page size | used to fill `MeasurementQuery::max_batch_size` |
| `get_station_image(id)` | optional image bytes | default returns `Ok(None)`; report `image_sha256` per station |
| `attach_persistent_state(handle)` | optional scoped state | override only for stateful providers |
| `attach_provider_messages(sink)` | optional message sink | override to emit scoped messages |

The core owns all entity identity (`Uuid`s, `data_source_id` links); the adapter
only reports stable external ids. Do not return database/DTO types — only the
port's record types and `ProviderError`.

Every [`MeasurementRecord`](backend/src/core/domain/data_source/provider_port.rs:128)
must declare `resolution_seconds` — the length in seconds of the interval its
count covers (an open value: 300 = 5 min, 900 = 15 min, 3600 = 1 h, ...). Drop
any observation whose duration you cannot determine (a re-import recovers it);
never guess a duration. Calendar-anchored resolutions (daily/weekly) additionally
set `interval_end` to the exact, DST-aware interval end.

### 2. Read configuration from the data source

A provider's config is declared in the `[data_sources.provider]` TOML section:

```toml
[[data_sources]]
name = "My source"

[data_sources.provider]
type = "my_provider"
log_level = "WARNING"        # optional; min severity to persist (default WARNING)

[data_sources.provider.vars]
url = "https://example.com/data.zip"
some_number = "42"           # vars are strings; parse them in the adapter
```

- `type` — must match the provider type registered in the factory (step 3).
- `log_level` — one of `TRACE`, `DEBUG`, `INFO`, `WARNING`, `ERROR`; validated at
  config-parse time. The **core** drops messages below this level, so the
  adapter does not need to filter — it only emits correct severities (see
  step 4). Default `WARNING`.
- `vars` — arbitrary string key/values; each adapter parses its own (e.g. the
  Münster adapter reads `url`, `max_measurement_batch_size`,
  `max_measurement_timeframe_hours`, `cache_duration` — see
  [`adapter.rs`](backend/src/adapter/driven/muenster_github/adapter.rs:82)).

Build the adapter in `new(&DataSourceConfiguration) -> Result<Self, ConfigError>`
and fail fast on missing/invalid required vars.

### 3. Register the provider type

Add a match arm to
[`DataProviderFactoryImpl::build`](backend/src/adapter/driven/data_provider_factory.rs:14):

```rust
if config.provider().provider_type() == MyProvider::provider_type() {
    Ok(Arc::new(MyProvider::new(config)?))
} else { ... }
```

Expose the type via an associated `provider_type()` returning the exact `type`
string.

### 4. Emit provider messages

To surface non-fatal conditions (and lifecycle tracing) without failing the
import, override `attach_provider_messages` and store the sink, then emit
concise one-line events (best-effort — failures are swallowed):

```rust
pub(crate) fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
    if let Some(sink) = self.messages.lock().unwrap().as_ref() {
        let _ = sink.provider_event_occurred(severity, message.as_ref());
    }
}
```

**Severity conventions** (least → most severe: `TRACE < DEBUG < INFO < WARNING <
ERROR`):

| Severity | Use for |
|---|---|
| `TRACE` | per-row/probe tracing (rare) |
| `DEBUG` | known, non-fatal data quirks (e.g. Münster's missing CSV column) |
| `INFO` | lifecycle one-liners ("archive downloaded", "cache refreshed") |
| `WARNING` | non-fatal anomalies an operator should see |
| `ERROR` | conditions that warrant attention but do not abort |

The **core** enforces the policy, so adapters stay simple:

- Events below the provider's `log_level` are **dropped** (default `WARNING`
  keeps only `WARNING`/`ERROR`, so use `DEBUG`/`INFO` for noise you do not want
  persisted by default).
- Only the first **1000** events per data source are persisted; on overflow the
  core records a single truncation `WARNING` (and prints it to stdout).
- The database additionally caps at **1001** rows per data source (migration
  `V13`), so the invariant holds even if the core is bypassed.

Genuine IO/parse failures should still return `ProviderError` and fail the job —
only *known quirks* become messages.

### 5. Persistent state (optional)

For stateful providers (e.g. an archive cache), override `attach_persistent_state`
and use the scoped [`PersistentStateAccess`](backend/src/core/domain/data_source/provider_port.rs:191)
(`load`/`store`/`delete`/`clear`). The handle is handed over after the data
source row exists (two-phase handover) and is scoped to the provider's data
source. See the Münster cache lifecycle in
[`adapter.rs`](backend/src/adapter/driven/muenster_github/adapter.rs:198).

### 6. Health

`check_health` feeds the readiness endpoint. Return `HealthStatus::Up` when the
upstream is reachable/authenticated and `Down` otherwise. The core wraps it in a
[`ProviderHealthIndicator`](backend/src/core/domain/health/provider_health_indicator.rs:1).

### 7. Tests and gates

- Unit-test the parser/import logic with in-memory mocks and fixture files (see
  [`muenster_github/tests.rs`](backend/src/adapter/driven/muenster_github/tests.rs:1)).
- Keep **core** coverage ≥ 95% and overall ≥ 80% (`make coverage`); add tests
  for new behavior instead of lowering thresholds.
- Frontend unit tests (Vitest under jsdom, covering the whole `frontend/src`
  including React components) run with `make test-unit`; the whole-src line
  coverage is kept at ≥ 80 % via `make coverage` and `make test-unit-coverage`
  (thresholds in [`frontend/vitest.config.ts`](frontend/vitest.config.ts)). The
  produced lcov is reported to Codecov under the `frontend` flag, which — like
  the `backend` flag — is mandated by the ≥ 80 % project status in
  [`codecov.yml`](codecov.yml) (see [`agents.md`](agents.md)).
- Run all gates before finishing: `make check`, `make test`/`make test-rest`,
  `make coverage` (see [`agents.md`](agents.md:25)).

## Definition of done

See [`agents.md`](agents.md:92): a numbered plan in
[`plans/`](plans) registered in [`plans/README.md`](plans/README.md), all gates
green, and the touched docs updated.
