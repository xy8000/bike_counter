# 142 - Backend dependency + Docker base-image bump

Status: implemented

## Problem

On the `chore/frontend-dependency-bump` branch the user bumped several direct
crate requirements in [`backend/Cargo.toml`](../backend/Cargo.toml:7):

| Crate | was | now |
| --- | --- | --- |
| `arrow` | `59.3.0` | `60.0.0` |
| `parquet` | `59.3.0` | `60.0.0` |
| `toml` | `1.1.6` | `1.1.6+spec-1.1.0` |
| `utoipa` | `5.5.0` | `6.0.0` |
| `utoipa-swagger-ui` | `9.0.2` | `10.0.1` |
| `testcontainers` | `0.27.3` | `0.28.0` |

[`backend/Cargo.lock`](../backend/Cargo.lock:1) was **not** regenerated, and the
Docker base images remain pinned to their old digests. The task is to make the
bump consistent, resolve the incompatibilities it introduces, refresh
[`backend/Cargo.lock`](../backend/Cargo.lock:1) and bring the digest-pinned base
images in [`backend/Dockerfile`](../backend/Dockerfile:4) and
[`frontend/Dockerfile`](../frontend/Dockerfile:4) up to date.

## Findings

1. **`testcontainers = "0.28.0"` is incompatible with the tree.**
   [`testcontainers-modules` 0.15.0](https://crates.io/crates/testcontainers-modules/0.15.0)
   is still the latest release and pins `testcontainers = "^0.27.0"` (verified via
   the crates.io dependency API). A direct `0.28.0` would split the tree into two
   `testcontainers` versions, and the repository tests hold a
   `testcontainers::Container<testcontainers_modules::postgres::Postgres>` whose
   `Image`/`Container` bounds cannot mix majors. Revert to `0.27.3`, exactly as
   [`plans/137`](137_dependency_upgrade_release_notes_plan.md:39) already
   documented and the in-file comment states.
2. **`toml = "1.1.6+spec-1.1.0"` re-introduces a manifest warning.** Cargo ignores
   semver build metadata in a *requirement* (it belongs only in the crate's own
   version, which the lock still records), so this is the exact regression fixed
   before in [`plans/113`](113_dependency_version_bump_compat_plan.md:32) and
   [`plans/137`](137_dependency_upgrade_release_notes_plan.md:53) — drop the
   metadata: `"1.1.6"`.
3. **`arrow`/`parquet` 60.0.0, `utoipa` 6.0.0 and `utoipa-swagger-ui` 10.0.1 are
   the current stable releases** (verified via crates.io). They are retained, but
   the code must be checked against the major bumps in `utoipa`.
4. **Docker base images are digest-only pins**
   ([`plans/127`](127_sonar_open_issues_plan.md:12)). Refreshing them means
   resolving the *current* manifest-list digest of the same tags
   (`rust:1-alpine`, `alpine:3.24`, `node:24-alpine`, `nginx:1.31-alpine`).

### Current vs. refreshed image digests

| Dockerfile | Tag | Current digest | Refreshed digest |
| --- | --- | --- | --- |
| [`backend/Dockerfile`](../backend/Dockerfile:4) | `rust:1-alpine` | `a5163321…` | `7cc1c22d…` |
| [`backend/Dockerfile`](../backend/Dockerfile:33) | `alpine:3.24` | `79ff19e9…` | `294b683c…` |
| [`frontend/Dockerfile`](../frontend/Dockerfile:4) | `node:24-alpine` | `333f6b3e…` | `ebfe2f90…` |
| [`frontend/Dockerfile`](../frontend/Dockerfile:16) | `nginx:1.31-alpine` | `bd43d3d8…` | `1ed1b0e1…` |

## Changes

1. [`backend/Cargo.toml`](../backend/Cargo.toml:45): `testcontainers`
   `0.28.0` → `0.27.3` (compatible with `testcontainers-modules` 0.15.0).
2. [`backend/Cargo.toml`](../backend/Cargo.toml:34): `toml`
   `"1.1.6+spec-1.1.0"` → `"1.1.6"` (no build metadata in the requirement).
3. Regenerate [`backend/Cargo.lock`](../backend/Cargo.lock:1) (`cargo update`) so
   `arrow`/`parquet` 60, `utoipa` 6 and `utoipa-swagger-ui` 10 resolve.
4. [`backend/Dockerfile`](../backend/Dockerfile:4): refresh the `rust:1-alpine`
   builder and `alpine:3.24` runtime digests.
5. [`frontend/Dockerfile`](../frontend/Dockerfile:4): refresh the
   `node:24-alpine` builder and `nginx:1.31-alpine` runtime digests.
6. Fix any source breakage the `utoipa` 6 / `arrow` 60 / `parquet` 60 bumps
   surface.

## Outcome

All changes applied and verified:

- `cargo update` refreshed [`backend/Cargo.lock`](../backend/Cargo.lock:1) to
  `arrow`/`parquet` 60.0.0, `utoipa` 6.0.0 (`utoipa-gen` 6.0.1) and
  `utoipa-swagger-ui` 10.0.1, and to the compatible `testcontainers` 0.27.3.
- `cargo check --all-targets` green; the major bumps needed **no** source
  changes.
- `make check` green (rustfmt + clippy `-D warnings` + Prettier + cargo audit,
  no manifest warning).
- `make test-rest` green (128) and `make test` green (767, exercising the
  `testcontainers` pin against a Docker Postgres).
- Refreshed base-image digests resolve to their tags and both
  [`backend/Dockerfile`](../backend/Dockerfile:1) and
  [`frontend/Dockerfile`](../frontend/Dockerfile:1) lint clean
  (`docker build --check`).
- `docker compose build` built **both** images successfully against the new
  digests (backend compiled end-to-end with the refreshed `rust:1-alpine`
  toolchain and the `alpine:3.24` runtime; frontend with `node:24-alpine`).

## Verification

- `cargo update` + `cargo check --all-targets` green, no manifest warning.
- `make check` green (rustfmt + clippy `-D warnings` + Prettier + cargo audit).
- `make test-rest` green.
- `make test` green (Docker Postgres repository tests, exercising the
  `testcontainers` pin).
- `docker build` of both images succeeds with the refreshed digests.

## Results

| Command | Result |
| --- | --- |
| `cargo check --all-targets` | green |
| `make check` | green (fmt, clippy, Prettier, audit) |
| `make test-rest` | 128 passed |
| `make test` | 767 passed |
| `docker build --check` | both clean |
| `docker compose build` | both images built |

## Definition of done

- [x] [`backend/Cargo.toml`](../backend/Cargo.toml:45) `testcontainers` reverted to `0.27.3`
- [x] [`backend/Cargo.toml`](../backend/Cargo.toml:34) `toml` requirement without build metadata
- [x] [`backend/Cargo.lock`](../backend/Cargo.lock:1) regenerated
- [x] [`backend/Dockerfile`](../backend/Dockerfile:4) base-image digests refreshed
- [x] [`frontend/Dockerfile`](../frontend/Dockerfile:4) base-image digests refreshed
- [x] `make check` green
- [x] `make test-rest` green
- [x] `make test` green
- [x] plan file status/checklist kept current
