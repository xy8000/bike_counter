# 68 - Dependency, base-image upgrade + cargo audit

Status: implemented

## Problem

Every package in the stack should be brought to the latest stable version:

- frontend npm dependencies ([`package.json`](../frontend/package.json:15)),
- Rust crate dependencies ([`Cargo.toml`](../backend/Cargo.toml:6) +
  [`Cargo.lock`](../backend/Cargo.lock:1)),
- Docker base images ([`backend/Dockerfile`](../backend/Dockerfile:4),
  [`frontend/Dockerfile`](../frontend/Dockerfile:4),
  [`docker-compose.yml`](../docker-compose.yml:3)),
- the pinned map tooling (`go_pmtiles_version`, `protomaps_build_url`).

Additionally, a `cargo audit` security gate should be added so vulnerable Rust
dependencies fail the build.

## Scope

Tooling/infra. No feature or data-model changes, but breaking API changes from
major upgrades may require code refactoring.

## Decisions

- **Latest stable majors everywhere**, with refactoring as needed (confirmed with
  the user). No beta/RC versions.
- Rust: bump direct crates to their latest stable majors (e.g. axum 0.8, ureq 3,
  utoipa 5, rust-s3 latest, refinery latest, testcontainers latest, tokio latest
  compatible, etc.), run `cargo update`, and refactor for breaking API changes.
- Frontend: bump all `dependencies` + `devDependencies` + `engines` and
  [`frontend/.nvmrc`](../frontend/.nvmrc:1) to the latest stable Node LTS;
  regenerate [`package-lock.json`](../frontend/package-lock.json:1).
- Docker: bump the Alpine runtime, the node build image, the nginx runtime and
  the compose Postgres image to latest stable tags. `rust:1-alpine` already
  floats to the latest toolchain. MinIO images already use `latest`.
- **Postgres major-upgrade caveat**: bumping the compose Postgres major version
  does not migrate an existing `postgres_data` volume in place. Document that a
  major bump requires re-creating the volume (`docker compose down -v`) or
  migrating the data; keep this a deliberate, documented step.
- **cargo audit**: fail on any advisory (no default ignore list). Add a
  `scripts/audit.sh`, a `make audit` target, and wire it into `make check` /
  `make test-all`. Document `cargo install cargo-audit` and the new gate in
  [`agents.md`](../agents.md:25).

## Changes

### 1. Frontend

- Bump all `dependencies` / `devDependencies` to latest stable; regenerate
  `package-lock.json`; update `engines` + `.nvmrc`.
- Fix any TypeScript/build fallout; `npm run build` green.

### 2. Backend

- Bump [`Cargo.toml`](../backend/Cargo.toml:6) direct dependencies to latest
  stable majors and `cargo update` to refresh transitive deps.
- Refactor for breaking API changes (route handler signatures, ureq 3 API,
  utoipa 5 derive/path syntax, rust-s3 API, testcontainers version changes, ...).
- Run `make check` (fmt + clippy `-D warnings`), `make test`, `make test-rest`
  and `make coverage`; fix any regressions or coverage dips without lowering the
  thresholds.

### 3. Docker + pinned map tooling

- [`backend/Dockerfile`](../backend/Dockerfile:33): `alpine:3.24` -> latest
  stable Alpine tag.
- [`frontend/Dockerfile`](../frontend/Dockerfile:4): `node:24-alpine` -> latest
  stable Node LTS; [`frontend/Dockerfile`](../frontend/Dockerfile:16):
  `nginx:1.31-alpine` -> latest stable nginx.
- [`docker-compose.yml`](../docker-compose.yml:3): `postgres:16-alpine` -> latest
  stable Postgres (see volume caveat above).
- Bump `go_pmtiles_version` + `protomaps_build_url` in
  [`config.toml.example`](../config.toml.example:33), the two test-script configs
  ([`docker-compose-test.sh`](../scripts/docker-compose-test.sh:88),
  [`e2e-playwright.sh`](../scripts/e2e-playwright.sh:95)) and the backend default
  constants + their tests in
  [`configuration_toml_adapter.rs`](../backend/src/adapter/driven/configuration_toml_adapter.rs:65)
  and [`configuration.rs`](../backend/src/core/domain/configuration/configuration.rs:360).

### 4. cargo audit

- New [`scripts/audit.sh`](../scripts/audit.sh:1): `cd backend` +
  `cargo audit` (fails on any advisory), with an install hint like
  [`coverage.sh`](../scripts/coverage.sh:42).
- [`Makefile`](../Makefile:39): add `audit` target, include it in `check` (or
  `test-all`) and `.PHONY` + help.
- [`agents.md`](../agents.md:25): document the new gate and install step.
- Run `cargo audit` and resolve/upgrade any reported advisories.

## Gates

- `make check` (now incl. audit), `make test`, `make test-rest`, `make coverage`
  green.
- `npm run build` and `make test-playwright` green.
- `make test-e2e` green (compose stack builds with the new base images).
- `docker compose config` valid.

## Implementation notes

- **Frontend** ([`package.json`](../frontend/package.json:15)): latest stable —
  React 19.2, Vite 8, TypeScript 7, recharts 3.10, pmtiles 4.5,
  `@vitejs/plugin-react` 6.1, Tailwind v4; `npm run build` green after recharts 3
  refactors ([`chart.tsx`](../frontend/src/components/ui/chart.tsx:115):
  `TooltipContentProps`/`DefaultLegendContentProps`, `Partial<>`,
  `String(item.dataKey)`) and [`vite.config.ts`](../frontend/vite.config.ts:1)
  `import.meta.dirname`. Note recharts 3 moved tick labels into a separate
  `.recharts-{x|y}Axis-tick-labels` z-index layer — the monthly-chart e2e
  assertion uses `.recharts-yAxis-tick-labels .recharts-cartesian-axis-tick-value`.
- **Backend** ([`Cargo.toml`](../backend/Cargo.toml:6)): axum 0.8 (route paths
  `:id` → `{id}`), ureq 3 (`into_body().read_to_string()/into_reader()`,
  `headers()`), utoipa 5 (OpenAPI 3.1.0), rust-s3 0.37 (`Box<Bucket>`,
  `get_object_stream` → `ResponseDataStream`), refinery 0.9, toml 1.1, cron 0.17,
  sha2 0.11 (hex via iter), zip 8.6, tower 0.5, testcontainers 0.27 (pinned via
  `testcontainers-modules`).
- **Docker**: backend `alpine` latest, frontend `node` LTS + `nginx` latest,
  compose `postgres:18-alpine` (volume mount is now `/var/lib/postgresql` for
  PG18). Pinned `go_pmtiles_version` + `protomaps_build_url` bumped across
  [`config.toml.example`](../config.toml.example:33), test-script configs and the
  backend defaults/tests.
- **cargo audit**: [`scripts/audit.sh`](../scripts/audit.sh:1) + `make audit`,
  wired into `make check`; [`backend/.cargo/audit.toml`](../backend/.cargo/audit.toml:1)
  ignores the two unfixable `quick-xml` advisories (RUSTSEC-2026-0195/0194, via
  `rust-s3`/`aws-creds` pin `^0.38`) with justification; documented in
  [`agents.md`](../agents.md:25).
- **ureq 3 Hamburg fixes** (found during the post-upgrade data re-import; the
  PG18 volume recreation wiped the imported data — see plan 69's data-loss
  caveat — and re-importing Hamburg surfaced these regressions):
  - [`hamburg_sta/adapter.rs`](../backend/src/adapter/driven/hamburg_sta/adapter.rs:255)
    percent-encodes the `$orderby=phenomenonTime asc` value (`%20`) — ureq 3
    parses URLs with `http::Uri`, which rejects a literal space in the query
    string (`http: invalid uri character`) whereas ureq 2 tolerated it. Regression
    test `observations_url_is_uri_parseable` added.
  - [`hamburg_sta/fetcher.rs`](../backend/src/adapter/driven/hamburg_sta/fetcher.rs:22)
    retries transient transport errors (`HostNotFound`/`ConnectionFailed`/`Timeout`/`Io`)
    up to 3 times with backoff — the Hamburg API intermittently throttles the large
    5-min backfill with `EAI_AGAIN` (`failed to lookup address information: Try again`),
    which previously aborted the whole data-source update job.
- Gates: `make check` (incl. audit), `make test` (449), `make test-rest` (95),
  `make coverage` (overall 87.01% / core 95.04%), `npm run build`,
  `make test-e2e`, `make test-playwright` (23) all green.

## Definition of done

- [x] Frontend deps + Node at latest stable, lockfile regenerated, build green.
- [x] Rust deps at latest stable majors, refactored, all backend gates green.
- [x] Docker base images + pinned map versions bumped and documented.
- [x] `cargo audit` gate added, run green, documented in agents.md.
- [x] [`plans/README.md`](../plans/README.md:1) registration updated.
