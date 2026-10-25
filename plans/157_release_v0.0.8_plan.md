# 157 - Release v0.0.8 to GitHub and Docker Hub

Status: implemented

## Problem

The work that landed on `main` since `v0.0.7` — the Hessen Mobil import fix
(plan 156) — has not been released. The next version after the released
`v0.0.7` is a **patch** release: `v0.0.8`.

## Current git state (verified from the working tree)

- The latest release tag is `v0.0.7`.
- `main` is at `58ed8cc` (`fix: surface Postgres errors and refresh channel
  names (plan 156)`), the rebased-and-merged plan-156 commit.
- The version strings across the repo are still `0.0.7`; this release bumps them
  to `0.0.8`.

## Delta (v0.0.7 -> `HEAD`)

- [plan 156](156_hessen_import_db_error_plan.md) — the Hessen Mobil import no
  longer fails with the opaque `Database("db error")`: Postgres repositories now
  surface the real SQLSTATE / message / constraint, and disambiguated station
  names propagate to their channels on re-sync. Two drifting/flaky Playwright
  e2e specs were also fixed (relative individual range + retried map void-click).

The release infrastructure from
[plan 119](119_release_v0.0.1_plan.md) is version-agnostic:
[`release.yml`](../.github/workflows/release.yml) derives the `0.0.8` + `latest`
tags from the pushed `v0.0.8` tag. Only the version strings and pinned image
tags need bumping.

## Approach

Follow the exact `v0.0.1` .. `v0.0.7` release flow. The prep is committed on a
`release/v0.0.8-release-prep` branch off `main`, then merged to `main` via PR
before the annotated `v0.0.8` tag is pushed.

### 1. Version bump `0.0.7` -> `0.0.8`

- [`backend/Cargo.toml`](../backend/Cargo.toml:3) — `version = "0.0.8"`.
- [`backend/Cargo.lock`](../backend/Cargo.lock:481) — the `[[package]] name =
  "bike_counter"` entry `version = "0.0.8"` (only the top-level package, never a
  dependency entry).
- [`frontend/package.json`](../frontend/package.json:4) — `"version": "0.0.8"`.
- [`frontend/package-lock.json`](../frontend/package-lock.json:3) and
  [`frontend/package-lock.json`](../frontend/package-lock.json:9) — the two
  top-level `version` fields (`0.0.8`); never touch `node_modules/*` entries.
- [`docker-compose.yml`](../docker-compose.yml:29) — backend image
  `xy8000/bike-counter-backend:0.0.8`.
- [`docker-compose.yml`](../docker-compose.yml:99) — frontend image
  `xy8000/bike-counter-frontend:0.0.8`.
- [`README.md`](../README.md:84) and [`README.md`](../README.md:103) — prose
  references to the pinned image names and release tags.
- [`.github/workflows/release.yml`](../.github/workflows/release.yml:9) —
  refresh the `0.0.7` examples in the doc comments (cosmetic; workflow logic is
  untouched).

### 2. Gates

Full CI gates run on the PR (`make check`, `make test`, `make coverage`,
`make test-playwright`).

### 3. Cut the release (owner actions)

- Push this branch and open/merge the PR to `main`.
- From the merged `main`:
  `git tag -a v0.0.8 -m "Bike-Counter 0.0.8"` then `git push origin v0.0.8`.
- The push triggers [`release.yml`](../.github/workflows/release.yml), which
  builds both multi-arch images, pushes `xy8000/bike-counter-backend:0.0.8`
  / `:latest` and `xy8000/bike-counter-frontend:0.0.8` / `:latest`,
  cosign-signs them, and creates the GitHub Release.

## Definition of done

- [x] Plan file in [`plans/`](.) created
- [x] Version bumped to `0.0.8` in `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`
- [x] `docker-compose.yml` image tags and [`README.md`](../README.md) prose updated to `0.0.8`
- [x] [`release.yml`](../.github/workflows/release.yml) doc-comment examples refreshed to `0.0.8`
- [x] Prep committed and pushed on `release/v0.0.8-release-prep`
- [x] PR merged to `main` — owner action
- [x] Annotated tag `v0.0.8` pushed — owner action
- [x] Release workflow completed green for `v0.0.8` — verified CI
- [x] Docker Hub shows `xy8000/bike-counter-backend:0.0.8` / `:latest` and `xy8000/bike-counter-frontend:0.0.8` / `:latest` — verified CI
- [x] GitHub Release `v0.0.8` exists with release notes — published 2026-10-24
