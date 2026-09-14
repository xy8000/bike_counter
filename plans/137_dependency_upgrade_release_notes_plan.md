# 137 - Dependency upgrade release-note review and compatibility fixes

Status: implemented

## Problem

Dependency versions were bumped in both manifests on the
`feat/update-dependencies` branch. This plan reviews the release notes of the
bumped packages, applies the changes the upgrades actually require, and reverts
the bumps that are incompatible with the rest of the tree.

Bumped crates ([`backend/Cargo.toml`](../backend/Cargo.toml:7)):

- axum 0.8 → 0.8.9, bytes 1.0 → 1.12.1, chrono 0.4 → 0.4.45,
  chrono-tz 0.10 → 0.10.4, cron 0.17 → 0.17.0, csv 1.4 → 1.4.0,
  flate2 1.1 → 1.1.10, futures 0.3 → 0.3.34, include_dir 0.7 → 0.7.4,
  openssl 0.10 → 0.10.81, postgres 0.19 → 0.19.14, r2d2 0.8 → 0.8.10,
  r2d2_postgres 0.18 → 0.18.2, refinery 0.9 → 0.9.2, rust-s3 0.37 → 0.37.2,
  serde 1.0 → 1.0.229, serde_json 1.0 → 1.0.151, sha2 0.11 → 0.11.0,
  tar 0.4 → 0.4.46, tokio 1.0 → 1.53.1, tokio-util 0.7 → 0.7.19,
  toml 1.1.5 → 1.1.6+spec-1.1.0, ureq 3.4.1 → 3.4.2, utoipa 5.5 → 5.5.0,
  utoipa-swagger-ui 9.0 → 9.0.2, uuid 1.0 → 1.26.1, zip 8.6 → 8.6.0,
  testcontainers 0.27.3 → 0.28.0, testcontainers-modules 0.15 → 0.15.0,
  tower 0.5 → 0.5.3, http-body-util 0.1 → 0.1.5.

Bumped npm packages ([`frontend/package.json`](../frontend/package.json:21)):

- lucide-react 1.41.0 → 1.46.0, maplibre-gl 6.7.0 → 6.9.1,
  react/react-dom 19.2.8 → 19.3.0, tailwind-merge 3.6.0 → 3.7.0,
  @testing-library/dom 10.4.1 → 10.4.2, @types/node 26.4.1 → 26.5.1,
  @types/react 19.2.18 → 19.3.0, @types/react-dom 19.2.7 → 19.3.0,
  @vitest/coverage-v8 4.1.5 → 5.0.0, prettier 3.6.2 → 3.9.6,
  vite 8.2.2 → 8.3.0, vitest 4.1.5 → 5.0.0.

## Release-note findings

### Backend — two problems in the supplied bump

1. **`testcontainers` 0.28.0 is incompatible with the rest of the tree.**
   [`testcontainers-modules` 0.15.0](https://crates.io/crates/testcontainers-modules/0.15.0)
   (still the latest release) pins `testcontainers = "0.27.0"`
   (`^0.27`). The testcontainers-rs
   [0.28.0 release notes](https://github.com/testcontainers/testcontainers-rs/releases/tag/0.28.0)
   mark one breaking change — "Update bollard 0.20 to 0.21". Bumping the direct
   dependency to 0.28.0 would split the tree into two `testcontainers` versions
   (0.28.0 direct + 0.27.x for the module), and every repository test holds a
   `testcontainers::Container<testcontainers_modules::postgres::Postgres>`
   ([`backend/src/adapter/driven/postgres`](../backend/src/adapter/driven/postgres))
   — an `Image` impl from one version cannot satisfy the other version's
   `Container` bound. Revert to `0.27.3`, exactly as the in-file comment already
   documents.

2. **`toml = "1.1.6+spec-1.1.0"` puts semver build metadata back into a
   version requirement.** Cargo ignores `+spec-1.1.0` when matching a
   requirement and emits a manifest warning; the metadata belongs only in the
   crate's own version (the lock keeps `1.1.6+spec-1.1.0`). This is the exact
   issue fixed before in
   [`plans/113_dependency_version_bump_compat_plan.md`](../plans/113_dependency_version_bump_compat_plan.md:32):
   drop the metadata from the requirement — `"1.1.6"`.

The remaining crate bumps are patch-level within the same major/minor and carry
no breaking changes.

### Frontend — one manifest change plus verification

- **Vitest 5.0.0 requires Node.js 22 and Vite 6.4**
  ([release notes](https://github.com/vitest-dev/vitest/releases/tag/v5.0.0)).
  Vite is already 8.3.0 (fine), but [`frontend/package.json`](../frontend/package.json:8)
  still advertises `engines.node: "^20.19.0 || >=22.12.0"`, so a Node 20 install
  would pull a Vitest that cannot run. Tighten to `">=22.12.0"`. The rest of the
  repo already uses Node 24 (`.nvmrc` `24`, the pinned `node:24-alpine` builder
  digest, CI via `node-version-file`), so no other Node references change.
- Other Vitest 5 breaking changes (default `clearMocks`, removed deprecated
  entry points, inline `expect`, no ancestor config lookup) are verified against
  the suite below rather than pre-emptively patched.
- `react` 19.3.0 is additive (`<ViewTransition />`, `addTransitionType`) — no
  source change.
- `maplibre-gl` 6.8/6.9 are feature/bugfix releases; no API removals.
- `lucide-react` 1.41→1.46: the only renames are flip icons and a Swiss-franc
  deprecation — neither is imported by
  [`frontend/src`](../frontend/src), so no icon change is needed.
- `prettier` 3.7→3.9 has no breaking/deprecation changes; `make check` confirms
  formatting stays clean.

## Changes

1. [`backend/Cargo.toml`](../backend/Cargo.toml:43): `testcontainers`
   `0.28.0` → `0.27.3` (compatible with `testcontainers-modules` 0.15.0).
2. [`backend/Cargo.toml`](../backend/Cargo.toml:34): `toml`
   `"1.1.6+spec-1.1.0"` → `"1.1.6"` (no build metadata in the requirement).
3. [`frontend/package.json`](../frontend/package.json:8): `engines.node`
   `"^20.19.0 || >=22.12.0"` → `">=22.12.0"`.
4. Regenerate [`backend/Cargo.lock`](../backend/Cargo.lock:1) (`cargo update`)
   and [`frontend/package-lock.json`](../frontend/package-lock.json:1)
   (`npm install`) so both locks match the corrected manifests.
5. Run the gates and patch any breakage the Vitest 5 / prettier bumps surface.
6. [`CONTRIBUTING.md`](../CONTRIBUTING.md:36): update the Node prerequisite note
   to the Vitest 5 floor (Node ≥ 22.12) instead of the stale Vite 7 note.
7. [`frontend/e2e/flags.spec.ts`](../frontend/e2e/flags.spec.ts:42): the
   "map void click clears the selected flag" spec clicked the map void while the
   `flyTo` selection animation was still running, so MapLibre could swallow the
   click as a pan/zoom gesture and the `station` param stayed in the URL. Added
   the same moveend-driven URL-stability wait already used in
   [`frontend/e2e/detail.spec.ts`](../frontend/e2e/detail.spec.ts:204) before the
   void click (surfaced by the Playwright run against the upgraded tree).

## Files touched

| File | Change |
|---|---|
| [`backend/Cargo.toml`](../backend/Cargo.toml:43) | `testcontainers` reverted to `0.27.3` |
| [`backend/Cargo.toml`](../backend/Cargo.toml:34) | `toml` requirement without build metadata |
| [`frontend/package.json`](../frontend/package.json:8) | Node engine floor raised to `>=22.12.0` |
| [`backend/Cargo.lock`](../backend/Cargo.lock:1) | regenerated |
| [`frontend/package-lock.json`](../frontend/package-lock.json:1) | regenerated |
| [`CONTRIBUTING.md`](../CONTRIBUTING.md:36) | Node prerequisite note updated |
| [`frontend/e2e/flags.spec.ts`](../frontend/e2e/flags.spec.ts:42) | wait for fly-to settle before the void click |

## Definition of done

- [x] `make check` green (fmt + clippy `-D warnings` + prettier + cargo audit, no manifest warning)
- [x] `cargo check --all-targets` green (backend compiles against the corrected crates)
- [x] `npm run build` (tsc + vite) green
- [x] `npm run test:unit` green (Vitest 5) — 553 passed
- [x] `make test-rest` green — 128 passed
- [x] `make test` green (full suite incl. Docker Postgres repository tests) — 767 passed
- [x] `make test-playwright` green (real seeded Docker stack) — 74 passed
- [x] plan file status/checklist kept current
