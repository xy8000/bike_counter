# 141 - Frontend dependency version bump verification

Status: implemented

## Problem

[`frontend/package.json`](frontend/package.json) was manually bumped to newer
minor/patch ranges for nine packages, but [`frontend/package-lock.json`](frontend/package-lock.json)
and `frontend/node_modules` were **not** regenerated:

| Package | was | now |
| --- | --- | --- |
| `lucide-react` | `^1.46.0` | `^1.48.0` |
| `maplibre-gl` | `^6.9.1` | `^6.11.2` |
| `react-router-dom` | `^7.18.3` | `^7.18.4` |
| `@types/node` | `^26.5.1` | `^26.6.2` |
| `@vitest/coverage-v8` | `^5.0.0` | `^5.0.2` |
| `jsdom` | `^30.0.1` | `^30.1.1` |
| `prettier` | `^3.9.6` | `^3.9.9` |
| `vite` | `^8.3.0` | `^8.3.1` |
| `vitest` | `^5.0.0` | `^5.0.2` |

`npm ls --depth=0` reports all nine as `invalid` (installed version does not
satisfy the new range), and the lockfile is out of sync with `package.json`, so
`npm ci` — used by [`frontend/Dockerfile`](frontend/Dockerfile:9) and CI — would
fail.

## Goal

Make the version bump consistent and verified: refresh the lockfile, then prove
build, formatting, unit tests/coverage and the browser e2e suite still pass with
the new dependency versions.

## Changes

- Run `npm install` in [`frontend/`](frontend) to regenerate
  [`frontend/package-lock.json`](frontend/package-lock.json) and `node_modules`
  against the new ranges. All bumps stay within the same major version, so no
  source changes are expected; fix any that surface.

## Verification

- `npm ls --depth=0` clean (no `invalid` entries).
- `npm run build` (`tsc && vite build`) succeeds.
- `npm run format:check` (Prettier) passes.
- `npm run test:unit:coverage` passes (whole-`src` thresholds).
- `make check` green.
- `make test-playwright` green (exercises the maplibre-gl upgrade on the map).

## Outcome

`npm install` regenerated [`frontend/package-lock.json`](frontend/package-lock.json)
(`removed 3 packages, changed 31 packages`, 0 vulnerabilities) and all nine
packages now resolve to the requested versions. Everything is green:

- `npm ls --depth=0` — clean (no `invalid`).
- `npm run build` — `tsc` + `vite 8.3.1` build succeeds (2658 modules).
- `npm run format:check` — Prettier (3.9.9) clean.
- `make test-unit-coverage` — Vitest 5.0.2 + v8 coverage green.
- `make check` — fmt, clippy, Prettier, cargo audit green.
- `make test-playwright` — 74 passed; the frontend image rebuilt via `npm ci`
  (proving the lockfile is in sync) and the map (maplibre-gl 6.11.2) works.

Notes (non-blocking): `npm install` printed a transient `@vitest/coverage-v8`
peer-resolution warning that resolved correctly (both at 5.0.2), and the build
still reports the pre-existing >500 kB chunk-size hint.

## Definition of done

- [x] [`frontend/package-lock.json`](frontend/package-lock.json) regenerated and in sync
- [x] `npm ls --depth=0` clean
- [x] `npm run build` green
- [x] `npm run format:check` green
- [x] `make test-unit-coverage` green (coverage at/above threshold)
- [x] `make check` green
- [x] `make test-playwright` green
