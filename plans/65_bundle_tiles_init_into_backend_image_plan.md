# 65 - Bundle the tiles init into the backend startup (reuse go-pmtiles)

Status: implemented

## Problem

The `tiles` service in [`docker-compose.yml`](../docker-compose.yml) carries a
~75-line inline `entrypoint` shell script (download the pinned `go-pmtiles`
CLI, extract the worldwide backdrop + Germany detail from the Protomaps build,
merge them, and clean up). This bloats the compose file with logic that belongs
in the application, not in orchestration.

Constraints and owner decisions:

- open-source project with a **two-image limit** (frontend + backend);
- **no tile data baked into the image** — the backend downloads and builds the
  basemap at runtime;
- the orchestration lives **in the backend application code** (a driven
  adapter, run during the startup init phase) — no Docker init containers, no
  shell scripts;
- **reuse `go-pmtiles`** for the PMTiles work — do not reimplement the format;
- **no `SKIP_TILES`** — the application can only run with tiles;
- the Germany **bbox is hard-coded** for now;
- tiles are **refreshed on a cron schedule**, atomically (build into a separate
  file, then rename into place) so the running application stays online;
- the tile/map settings are grouped in a **`[maps]` TOML section**.

## Approach

### Configuration: a `[maps]` TOML table

Add a `MapsConfiguration` value object (like `AssetStorageConfiguration`) plus a
`maps` field and getters on [`Configuration`](../backend/src/core/domain/configuration/configuration.rs),
parsed by the TOML adapter:

```toml
[maps]
# CRON expression for the basemap refresh (default: every two months).
update_cron = "0 0 3 1 1,3,5,7,9,11 *"
# ShedLock-style max lifetime for the tiles update job in seconds (default: 2 hours).
update_max_lifetime_seconds = 7200
# Pinned Protomaps daily build (see maps.protomaps.com/builds). Bump periodically.
protomaps_build_url = "https://build.protomaps.com/20260829.pmtiles"
# Pinned go-pmtiles CLI version.
go_pmtiles_version = "1.31.2"
```

The Germany bbox stays a hard-coded code constant `(5.8, 47.2, 15.1, 55.1)`.
The output directory remains an operational env var `TILES_DIR` (default
`/data`, the compose mount).

### Driven adapter: `backend/src/adapter/driven/tiles_init/`

A synchronous `TilesInit` adapter that downloads the pinned `go-pmtiles` CLI
(from `maps.go_pmtiles_version`) and drives it via `ureq` +
`std::process::Command`:

- `ensure_available()` — called at startup: if `$TILES_DIR/map.pmtiles` is
  missing, build it (blocking); if present, do nothing. There is **no skip
  path** — the server cannot start without tiles.
- `update()` — build into a temp file (`map.pmtiles.tmp` in the same
  directory), then `std::fs::rename` it to `map.pmtiles` so nginx always serves
  either the old or the new complete archive, never a partial one.
- Shared build step: download/extract the CLI (cache it), run
  `pmtiles extract` for the worldwide z0–5 backdrop, `pmtiles extract` for the
  Germany bbox z6–15, and `pmtiles merge`, with retry on transient extract
  failures, then remove intermediate files. Run the subprocess with inherited
  stdout/stderr so the CLI's progress streams to the container logs (extraction
  takes minutes), and print an explicit step message before each stage.

### Scheduled update: `backend/src/core/application/tiles_update_service.rs`

Mirrors [`AssetCleanupService`](../backend/src/core/application/asset_cleanup_service.rs):

- a `TilesUpdateService` implementing `ScheduledJobPort` (ShedLock-style
  `run_if_due`), tracking itself as a `tiles_update` job (no migration needed —
  job types are free-form text);
- a domain driven port (e.g. `TilesProvisioningPort`) with
  `ensure_available()` / `update()`, implemented by the `TilesInit` adapter;
- registered with the generic cron scheduler in
  [`main.rs`](../backend/src/main.rs) next to the data-source update and asset
  cleanup schedulers.

### Startup wiring

- [`main.rs`](../backend/src/main.rs) calls `tiles_init.ensure_available()`
  synchronously during the init phase, before the HTTP server binds. DB
  migrations already run (via `create_pool`) before the server binds, so
  [`/health/ready`](../backend/src/adapter/driving/rest/handlers/health.rs)
  only becomes reachable once migrations **and** tiles are done.
- Parse a `tiles` subcommand at the top of `main` (after config load) so
  `make tiles` / `make tiles-update` can run just the build step and exit;
  forward args in [`backend/docker/entrypoint.sh`](../backend/docker/entrypoint.sh).

### Orchestration

- [`docker-compose.yml`](../docker-compose.yml): remove the `tiles` service and
  its inline entrypoint. The `backend` service gains a `./tiles:/data` bind
  mount (config.toml is already mounted). No tile env vars are needed — the
  settings live in `[maps]`. The `frontend` service waits on
  `backend: service_healthy`.
- [`Makefile`](../Makefile): `tiles` / `tiles-update` run the `tiles`
  subcommand through the backend image (`docker compose run --rm --no-deps
  backend tiles`); `run` drops its `tiles` prerequisite.
- Docs: [`tiles/README.md`](../tiles/README.md), root
  [`README.md`](../README.md), [`ToDo.md`](../ToDo.md).

## Consequences to note

- Existing `config.toml` files may add the `[maps]` table to override the
  defaults (bi-monthly cron, 2-hour max lifetime, pinned build URL + CLI
  version); the table is entirely optional.
- Without `SKIP_TILES`, the smoke test
  ([`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh)) can
  no longer skip the basemap; on a fresh checkout it builds tiles before the
  backend becomes ready (subsequent runs reuse `tiles/map.pmtiles`). The
  readiness wait and CI caching should account for this.
- A cron run with an unchanged `protomaps_build_url` reproduces the same
  snapshot; new map data is introduced by bumping the pinned URL in `[maps]`,
  and the next cron run applies it atomically.

## Removed

- The `tiles` compose service and its inline entrypoint shell script.
- `SKIP_TILES`, `GERMANY_BBOX`, `PROTOMAPS_BUILD_URL`, `GO_PMTILES_VERSION`
  env vars (bbox is a code constant; the rest live in `[maps]`).

## Validation

- `docker compose config` parses cleanly; no `tiles` service remains.
- `docker compose build` produces only the backend image (frontend is
  separate); no third image is created.
- `make tiles` produces `tiles/map.pmtiles`; a second run is a no-op because the
  file exists.
- `docker compose up --build` boots the full stack; the frontend waits for the
  backend and then serves `/tiles/map.pmtiles` (PMTiles magic header readable
  by the browser).
- `tiles_update` job runs on its cron schedule and swaps the archive atomically
  (old file still served during the build).
- Unit tests for the adapter decision/command logic behind a subprocess seam,
  for `MapsConfiguration` parsing, and for `TilesUpdateService`; coverage gate
  stays green.
- Gates from [`agents.md`](../agents.md): `make check`, `make test` (or
  `make test-rest`), `make coverage`, and `make test-e2e`
  ([`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh)).

## Definition of done

- [x] `MapsConfiguration` + `[maps]` parsing added to `Configuration`, the TOML
      adapter, `config.toml.example`, and the test scripts' generated configs
- [x] `TilesInit` adapter in `backend/src/adapter/driven/tiles_init/` with
      `ensure_available()` / `update()` (atomic rename) and a hard-coded bbox
- [x] `TilesProvisioningPort` + `TilesUpdateService` (`tiles_update` scheduled
      job) added and tested
- [x] `main.rs` ensures tiles in the init phase; `tiles` subcommand added
- [x] `docker-compose.yml` removes the `tiles` service, mounts `./tiles` into
      the backend, and makes the frontend wait on backend readiness
- [x] `Makefile` updated; docs updated (`tiles/README.md`, `README.md`)
- [x] Validation steps above pass
