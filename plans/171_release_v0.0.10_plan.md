# 171 - Release v0.0.10 to GitHub and Docker Hub

Status: in progress

## Problem

The work that landed on `main` since `v0.0.9` has not been released. The next
version after the released `v0.0.9` is a **patch** release: `v0.0.10`.

## Current git state (verified from the working tree)

- The latest release tag is `v0.0.9` (annotated, points at `81aa708`,
  `release: prepare v0.0.9 publish (version bump, image tags, dependency
  refresh)`).
- `main` is at `4f49af2` (`feat(#164): harden HTTP edge, admin mutations,
  readiness and read paths`) — 12 commits ahead of `v0.0.9`.
- The version strings across the repo are still `0.0.9`; this release bumps them
  to `0.0.10`.
- The prep is committed on `release/v0.0.10-release-prep`, branched off the
  up-to-date `main`.

## Delta (v0.0.9 -> `main`)

- [plan 165 (pagination)](165_import_jobs_pagination_plan.md) — the BFF
  import-run history (`GET /api/bff/data-sources/{id}/imports`) accepts
  `offset`/`limit`, defaults to the newest 50 runs and returns `total` plus
  HATEOAS `next`/`prev`; the average duration stays computed over all finished
  runs. `ImportHistory` renders a page with Previous/Next, covered by Vitest and
  a Playwright spec (issue #48).
- [plan 165 (name collisions)](165_name_collisions_must_not_fail_imports_plan.md)
  — station/channel name collisions never fail an import any more
  (`V28__drop_name_uniqueness.sql` drops the uniqueness that produced the Hessen
  `23505` duplicate-key failure, issue #47); the upsert key is the natural
  `(data_source_id, external_id, channel_name)` triple.
- [plan 166](166_import_jobs_live_status_plan.md) — the Import jobs table shows
  the canonical `RUNNING` / `FINISHED` / `FAILED` status instead of a
  `Successful` boolean, and a running import's warning/error/measurement/station
  counters are persisted live; the page polls every 5 s while a run is running.
- [plan 167](167_playwright_map_popup_settle_plan.md) — the map exposes
  `data-map-moving` and the Playwright suite waits for the camera to settle
  before clicking a popup link (map-popup flake); the e2e Postgres healthcheck
  uses a TCP `pg_isready` so Compose no longer starts the backend during the
  temporary init server; the settings setters chain through a pending buffer.
- [plan 163 (readiness)](163_readiness_provider_decoupling_plan.md) —
  `/health/ready` only probes PostgreSQL and performs no external I/O;
  third-party reachability moved to the informational `/health/providers`
  (always `200`, cached, never gating readiness).
- [plan 163 (build time)](163_rust_build_time_optimization_plan.md) — Rust
  profiles (`debug = "line-tables-only"`, dependencies without debug info) plus
  trimmed codec features (`arrow`/`parquet`/`zip` default features off) cut build
  time and artifact size; `release` stays untouched.
- [plan 164 (dead code)](164_remove_crate_wide_dead_code_allow_plan.md) — the
  crate-wide `#![allow(dead_code)]` is gone together with the dead code it hid.
- [plan 164 (F1–F5)](164_f1_f5_hardening_plan.md) — HTTP middleware (request
  ids, trace spans, per-surface timeouts, body limit, panic isolation,
  concurrency limit), bearer-token admin authentication for the six mutating
  routes (`admin_token`, routes answer `404` when unset), an explicit database
  capacity model (`database_pool_max_size`,
  `database_connection_timeout_seconds`, `database_statement_timeout_seconds`,
  `http_max_concurrent_requests`), BFF read-path aggregation/index work and the
  nginx security/gzip headers. The backend port is published on loopback only.
- [plan 170](170_async_db_driver_evaluation_plan.md) — evaluation only, no code.

The release infrastructure from [plan 119](119_release_v0.0.1_plan.md) is
version-agnostic: [`release.yml`](../.github/workflows/release.yml) derives the
`0.0.10` + `latest` tags from the pushed `v0.0.10` tag. Only the version strings
and pinned image tags need bumping.

## Approach

Follow the exact `v0.0.1` .. `v0.0.9` release flow. The prep is committed on a
`release/v0.0.10-release-prep` branch off `main`, merged to `main`
(fast-forward, as the branch only adds the prep commit), and then the annotated
`v0.0.10` tag is pushed.

### 1. Version bump `0.0.9` -> `0.0.10`

- [`backend/Cargo.toml`](../backend/Cargo.toml:3) — `version = "0.0.10"`.
- [`backend/Cargo.lock`](../backend/Cargo.lock:411) — the `[[package]] name =
  "bike_counter"` entry `version = "0.0.10"` (only the top-level package, never a
  dependency entry).
- [`frontend/package.json`](../frontend/package.json:4) — `"version": "0.0.10"`.
- [`frontend/package-lock.json`](../frontend/package-lock.json:3) and
  [`frontend/package-lock.json`](../frontend/package-lock.json:9) — the two
  top-level `version` fields (`0.0.10`); never touch `node_modules/*` entries.
- [`docker-compose.yml`](../docker-compose.yml:34) — backend image
  `xy8000/bike-counter-backend:0.0.10`.
- [`docker-compose.yml`](../docker-compose.yml:112) — frontend image
  `xy8000/bike-counter-frontend:0.0.10`.
- [`README.md`](../README.md:84) and [`README.md`](../README.md:103) — prose
  references to the pinned image names and release tags.
- [`backend/src/adapter/driving/bff/dto.rs`](../backend/src/adapter/driving/bff/dto.rs:221)
  — refresh the `VersionDto` `#[schema(example = "0.0.10")]` so the OpenAPI docs
  show the released version (doc-only; the endpoint serves `CARGO_PKG_VERSION`).
- [`.github/workflows/release.yml`](../.github/workflows/release.yml:9) —
  refresh the `0.0.9` examples in the doc comments (cosmetic; workflow logic is
  untouched).

No dependency refresh is part of this release: the backend and frontend
manifests/lockfiles are unchanged since `v0.0.9` apart from the new first-party
code, and `cargo audit` must stay green as-is.

### 2. Upgrade notes (breaking-ish for operators)

These are documented in the plan and covered by the README, which the F1–F5 work
already updated:

- `admin_token` is new and **optional**: the six data-mutating `/api/v1` routes
  are only registered (and then require `Authorization: Bearer <token>`) when it
  is configured; without it they answer `404`. Write clients must set it to keep
  mutating.
- The backend port is now published on `127.0.0.1:8080` only (the browser talks
  to the API through nginx); remote Swagger-UI access needs a tunnel or a
  compose override.
- Migration `V28__drop_name_uniqueness.sql` runs automatically on startup and
  removes the station/channel name uniqueness constraints.
- New optional capacity keys: `database_pool_max_size`,
  `database_connection_timeout_seconds`,
  `database_statement_timeout_seconds`, `http_max_concurrent_requests` (all have
  defaults, see [`config.toml.example`](../config.toml.example:6)).

### 3. Gates

- `make check` (fmt + clippy + prettier + cargo audit) — green.
- `make test-rest` — green.
- `make test-unit` — green.
- `make test` and `make coverage` run in CI on the release prep PR (Postgres
  test containers).

### 4. Cut the release

- Fast-forward `main` to the prep commit and push `main`.
- From the merged `main`:
  `git tag -a v0.0.10 -m "Bike-Counter 0.0.10"` then `git push origin v0.0.10`.
- The push triggers [`release.yml`](../.github/workflows/release.yml), which
  builds both multi-arch images, pushes `xy8000/bike-counter-backend:0.0.10`
  / `:latest` and `xy8000/bike-counter-frontend:0.0.10` / `:latest`,
  cosign-signs them, and creates the GitHub Release.

## Definition of done

- [x] Plan file in [`plans/`](.) created
- [ ] Version bumped to `0.0.10` in `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`
- [ ] `docker-compose.yml` image tags and [`README.md`](../README.md) prose updated to `0.0.10`
- [ ] `VersionDto` OpenAPI example and [`release.yml`](../.github/workflows/release.yml) doc comments refreshed to `0.0.10`
- [ ] `make check`, `make test-rest` and `make test-unit` green
- [ ] Prep committed on `release/v0.0.10-release-prep`
- [ ] `main` fast-forwarded to the prep commit and pushed
- [ ] Annotated tag `v0.0.10` created and pushed
- [ ] Release workflow completed green for `v0.0.10` — pending CI
- [ ] Docker Hub shows `xy8000/bike-counter-backend:0.0.10` / `:latest` and `xy8000/bike-counter-frontend:0.0.10` / `:latest` — pending CI
- [ ] GitHub Release `v0.0.10` exists with release notes — pending CI
