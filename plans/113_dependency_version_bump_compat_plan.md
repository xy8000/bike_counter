# 113 - Dependency version bump compatibility

Status: delivered (2026-09-06)

## Problem

Patch/minor dependency versions were bumped in both manifests:

- [`backend/Cargo.toml`](../backend/Cargo.toml:6): `toml` `1.1` -> `1.1.5`,
  `ureq` `3.4` -> `3.4.1` (pulls `ureq-proto` 0.6.1 -> 0.6.2),
  `testcontainers` `0.27` -> `0.27.3` (exact pin) —
  [`backend/Cargo.lock`](../backend/Cargo.lock:1) already refreshed.
- [`frontend/package.json`](../frontend/package.json:17): `@vis.gl/react-maplibre`
  8.1.2 -> 8.1.3, `lucide-react` 1.37.0 -> 1.41.0, `maplibre-gl` 6.6.0 -> 6.7.0,
  `@playwright/test` 1.62.1 -> 1.63.0, `@types/node` 26.4.0 -> 26.4.1,
  `@types/react-dom` 19.2.5 -> 19.2.7.
- [`docker-compose.yml`](../docker-compose.yml:1): comment-only cleanup (Postgres
  volume + network notes).

Task: make sure the code still compiles, type-checks and passes the gates against
these versions, upgrading the code only where the bumps actually require it.

## Investigation

- Frontend: `npm run build` (`tsc` + `vite build`) is green against the new
  versions — every `lucide-react` icon used
  (17 imports across [`frontend/src`](../frontend/src)) still exists in 1.41 and
  the `maplibre-gl` 6.7 / `@vis.gl/react-maplibre` 8.1.3 typings are compatible.
  No source change needed.
- Backend: `cargo check --all-targets` is green against the new crates. The only
  complaint is a **manifest warning**:
  `version requirement "1.1.5+spec-1.1.0" for dependency toml includes semver
  metadata which will be ignored`. Cargo never matches build metadata
  (`+spec-1.1.0`) in a version *requirement* (the crate's own version keeps the
  metadata, e.g. the lock shows `1.1.5+spec-1.1.0`), so the metadata in the
  requirement is noise and cargo recommends dropping it.
- The frontend [`package-lock.json`](../frontend/package-lock.json:1) had **not**
  been regenerated when `package.json` was edited, so the installed dependency
  tree was stale relative to the manifest (`npm install` now resyncs it).

## Changes

1. [`backend/Cargo.toml`](../backend/Cargo.toml:33): drop the semver build
   metadata from the `toml` requirement — `"1.1.5+spec-1.1.0"` -> `"1.1.5"` —
   which is what the requirement already meant and silences the cargo manifest
   warning on every backend command. The lock stays on `1.1.5+spec-1.1.0`.
2. [`frontend/package-lock.json`](../frontend/package-lock.json:1): regenerated
   via `npm install` so the lock matches the bumped `package.json`.
3. No Rust or TypeScript source changes are required — the bumps are API
   compatible (patch/minor).

## Gates

- `npm run build` (tsc + vite) green — verified.
- `cargo check --all-targets` green and warning-free — verified after the
  manifest cleanup.
- `make check` (rustfmt --check + clippy `-D warnings` + prettier --check +
  cargo audit) green.
- `make test-rest` green (in-memory REST tests, no Docker) — 128 passed.
- `make test` green (full suite incl. Docker Postgres repository tests) —
  739 passed.
- `make test-playwright` green (real seeded Docker stack, jobs disabled) —
  72 passed.

## Files touched

| File | Change |
|---|---|
| [`backend/Cargo.toml`](../backend/Cargo.toml:33) | `toml` requirement without build metadata |
| [`frontend/package-lock.json`](../frontend/package-lock.json:1) | regenerated against the bumped `package.json` |
| [`plans/README.md`](../plans/README.md:11) | registration |
| [`ToDo.md`](../ToDo.md:1) | progress note |

The `package.json` / `Cargo.toml` / `Cargo.lock` / `docker-compose.yml` version
bumps themselves were supplied by the user and are left as-is.

## Definition of done

- [x] Plan registered in [`plans/README.md`](../plans/README.md:11)
- [x] `make check` green (incl. no cargo manifest warning)
- [x] `make test-rest` green (128)
- [x] `make test` green (739)
- [x] `make test-playwright` green (72)
- [x] Frontend `npm run build` green
- [x] `ToDo.md` / plan docs updated
