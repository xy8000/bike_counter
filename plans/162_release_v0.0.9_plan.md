# 162 - Release v0.0.9 to GitHub and Docker Hub

Status: in progress

## Problem

The work that landed on `main` since `v0.0.8` has not been released. The next
version after the released `v0.0.8` is a **patch** release: `v0.0.9`.

## Current git state (verified from the working tree)

- The latest release tag is `v0.0.8` (annotated, points at `02cebef`,
  `release: prepare v0.0.8 publish (version bump + image tags)`).
- `origin/main` is at `dcbeeeb` (`refactor(#38): use optional chaining for empty
  import history check`); the local `main` was fast-forwarded to it.
- The version strings across the repo are still `0.0.8`; this release bumps them
  to `0.0.9`.
- The prep is committed on `release/v0.0.9-release-prep`, branched off the
  up-to-date `origin/main`.

## Delta (v0.0.8 -> `HEAD`)

- [plan 157](157_version_badge_plan.md) — header tag icon plus a version dialog
  that shows the frontend version (injected from `package.json` by Vite) and the
  backend version (`/api/bff/version`, `CARGO_PKG_VERSION`), covered by Vitest
  and a Playwright spec. (The feature was reverted once and re-applied;
  net effect on `main` is the feature itself plus its e2e spec.)
- [plan 158](158_increase_build_nice_factor_plan.md) — run the heavy build/test
  steps one `nice` level lower so local Docker builds stop starving the desktop.
- [plan 159](159_hessen_import_duplicate_key_plan.md) — Hessen Mobil import
  duplicate-key fix.
- [plan 160](160_vite_config_native_loader_plan.md) — Vite `configLoader:
  'native'` compatibility (no more `fileURLToPath`/`__dirname` in the configs),
  with the `readFileSync` choice documented.
- [plan 161](161_job_overview_average_runtime_plan.md) — data-source detail view
  shows the import jobs as a list-table plus the average import runtime
  (`V27__add_import_run_facts.sql`, import-run repository analytics, BFF DTOs,
  OpenAPI update, unit + Playwright coverage).
- Dependabot: `vite` 8.3.1 -> 8.3.4 and `undici` 8.10.2 -> 8.12.3 (frontend).

The release infrastructure from [plan 119](119_release_v0.0.1_plan.md) is
version-agnostic: [`release.yml`](../.github/workflows/release.yml) derives the
`0.0.9` + `latest` tags from the pushed `v0.0.9` tag. Only the version strings
and pinned image tags need bumping.

## Approach

Follow the exact `v0.0.1` .. `v0.0.8` release flow. The prep is committed on a
`release/v0.0.9-release-prep` branch off `main`, merged to `main` (fast-forward,
as the branch only adds the prep commit), and then the annotated `v0.0.9` tag is
pushed.

### 1. Version bump `0.0.8` -> `0.0.9`

- [`backend/Cargo.toml`](../backend/Cargo.toml:3) — `version = "0.0.9"`.
- [`backend/Cargo.lock`](../backend/Cargo.lock:481) — the `[[package]] name =
  "bike_counter"` entry `version = "0.0.9"` (only the top-level package, never a
  dependency entry).
- [`frontend/package.json`](../frontend/package.json:4) — `"version": "0.0.9"`.
- [`frontend/package-lock.json`](../frontend/package-lock.json:3) and
  [`frontend/package-lock.json`](../frontend/package-lock.json:9) — the two
  top-level `version` fields (`0.0.9`); never touch `node_modules/*` entries.
- [`docker-compose.yml`](../docker-compose.yml:29) — backend image
  `xy8000/bike-counter-backend:0.0.9`.
- [`docker-compose.yml`](../docker-compose.yml:99) — frontend image
  `xy8000/bike-counter-frontend:0.0.9`.
- [`README.md`](../README.md:84) and [`README.md`](../README.md:103) — prose
  references to the pinned image names and release tags.
- [`backend/src/adapter/driving/bff/dto.rs`](../backend/src/adapter/driving/bff/dto.rs:221)
  — refresh the `VersionDto` `#[schema(example = "0.0.9")]` so the OpenAPI docs
  show the released version (doc-only; the endpoint serves
  `CARGO_PKG_VERSION`).
- [`.github/workflows/release.yml`](../.github/workflows/release.yml:9) —
  refresh the `0.0.8` examples in the doc comments (cosmetic; workflow logic is
  untouched).

### 2. Dependency bump (shipped with this release)

`cargo audit` started failing on `main` (10 advisories published after
`v0.0.8`), so the release carries the dependency refresh:

- Backend manifests — `refinery` 0.9.2 -> 0.10.0, `rust-s3` 0.37.2 -> 0.38.0,
  `serde_json` 1.0.151 -> 1.0.154, `sha2` 0.11.0 -> 0.11.1, `tokio-util`
  0.7.19 -> 0.7.20, `toml` 1.1.6 -> 1.1.8, `ureq` 3.4.2 -> 3.5.0, `uuid`
  1.26.1 -> 1.29.0, `utoipa-swagger-ui` 10.0.1 -> 10.0.2, `zip` 8.6.0 ->
  9.1.0.
- Lockfile-only security bumps: `ctutils` 0.4.5, `rustls` 0.23.46,
  `rustls-webpki` 0.103.16, `tokio` 1.54.1, `webpki-roots` 1.0.11 — this
  clears RUSTSEC-2026-0346/0347/0348/0349 (rustls), 0381 (webpki-roots),
  0382 (rustls-webpki) and 0385 (ctutils).
- `zip` 9 (RUSTSEC-2026-0367/0368/0369) is reached through both the direct
  dependency and the `utoipa-swagger-ui` build-dependency; the 10.0.2 bump
  removes the last `zip` 8 in the graph. `ZipEntry::name()` is now fallible
  (`Result<Cow<'_, str>, ZipError>`), so
  [`muenster_github/adapter.rs`](../backend/src/adapter/driven/muenster_github/adapter.rs:393)
  interns the name and maps a decode error to `ProviderError::InvalidData`.
- `testcontainers` stays at 0.27.3: `testcontainers-modules` 0.15 (latest) pins
  `testcontainers ^0.27`, so a 0.28 bump cannot resolve.
- Frontend — Radix UI packages, `lucide-react` 1.61.0, `maplibre-gl` 6.16.0,
  `react`/`react-dom` 19.3.1, `@playwright/test` 1.64.1, `vite` 8.3.5,
  `vitest` + `@vitest/coverage-v8` 5.0.5, `jsdom` 30.1.3, `prettier` 3.9.10,
  `@types/node` 26.6.5, `@vitejs/plugin-react` 6.1.2,
  `@testing-library/user-event` 14.6.8; `package-lock.json` refreshed with
  `npm install`.
- [`VersionDialog.test.tsx`](../frontend/src/features/header/VersionDialog.test.tsx:8)
  now mocks the backend version as `1.2.3`: the old `0.0.9` mock collided with
  the injected frontend version and broke the unique `getByText` match.

### 3. Gates

- `make check` (fmt + clippy + prettier + cargo audit) — green.
- `make test` (847 backend tests incl. the Postgres containers) — green.
- `make test-rest` — green.
- `make test-unit` (576 Vitest tests) — green.
- `make coverage` — green.

Full CI gates also run on the PR (`make check`, `make test`, `make coverage`,
`make test-playwright`).

### 4. Cut the release

- Fast-forward `main` to the prep commit and push `main`.
- From the merged `main`:
  `git tag -a v0.0.9 -m "Bike-Counter 0.0.9"` then `git push origin v0.0.9`.
- The push triggers [`release.yml`](../.github/workflows/release.yml), which
  builds both multi-arch images, pushes `xy8000/bike-counter-backend:0.0.9`
  / `:latest` and `xy8000/bike-counter-frontend:0.0.9` / `:latest`,
  cosign-signs them, and creates the GitHub Release.

## Definition of done

- [x] Plan file in [`plans/`](.) created
- [ ] Version bumped to `0.0.9` in `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`
- [ ] `docker-compose.yml` image tags and [`README.md`](../README.md) prose updated to `0.0.9`
- [ ] `VersionDto` OpenAPI example and [`release.yml`](../.github/workflows/release.yml) doc comments refreshed to `0.0.9`
- [ ] `make check` and `make test-rest` green
- [ ] Prep committed and pushed on `release/v0.0.9-release-prep`
- [ ] `main` fast-forwarded to the prep commit and pushed
- [ ] Annotated tag `v0.0.9` created and pushed
- [ ] Release workflow completed green for `v0.0.9` — pending CI
- [ ] Docker Hub shows `xy8000/bike-counter-backend:0.0.9` / `:latest` and `xy8000/bike-counter-frontend:0.0.9` / `:latest` — pending CI
- [ ] GitHub Release `v0.0.9` exists with release notes — pending CI
