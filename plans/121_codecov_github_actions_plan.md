# 121 - Codecov coverage reporting via GitHub Actions (backend Rust + frontend Vitest)

Status: implemented

## Goal

Report test coverage to **Codecov** and show it as a README badge, wired into
**GitHub Actions** so every push / pull request re-measures and uploads the
coverage of both parts of the monorepo:

- the **backend** (Rust) — instrumented line coverage via `cargo-llvm-cov`
  (the tool [`scripts/coverage.sh`](../scripts/coverage.sh) already uses);
- the **frontend** (React/TS) — for which **no unit-test runner exists today**
  (only Playwright e2e). The owner suggested "Jest" and confirmed **Vitest is
  also possible**; Vitest is chosen because it runs on the existing Vite
  toolchain with no Babel/ts-jest plumbing.

The requested badge to add to [`README.md`](../README.md):

```markdown
[![codecov](https://codecov.io/gh/xy8000/bike_counter/graph/badge.svg?token=SVV43HD622)](https://codecov.io/gh/xy8000/bike_counter)
```

## Current state (verified)

- **No CI test/coverage workflow exists** — `.github/workflows/` contains only
  [`release.yml`](../.github/workflows/release.yml) (tag → Docker Hub + cosign +
  GitHub Release). So today nothing uploads coverage anywhere.
- **Backend coverage gate** exists and is enforced locally:
  [`scripts/coverage.sh`](../scripts/coverage.sh) → `make coverage` runs the
  whole suite with LLVM instrumentation and enforces *production-only* line
  coverage ≥ 80 % overall and ≥ 95 % in `src/core/`. The raw lcov lands in
  `backend/target/coverage/lcov.info`; the gate numbers are computed by an awk
  pass that **excludes** `#[cfg(test)]` regions and standalone test files —
  the lcov file itself still contains that test scaffolding.
- **Frontend has no unit tests**: `frontend/package.json` exposes only
  `test:e2e` (Playwright) and `devDependencies` have no unit runner; there are
  zero `*.test.*` files under `frontend/src`. Only pure-logic modules are good
  first Vitest targets (they do not need a DOM or maplibre).
- Codecov repo is `xy8000/bike_counter` (badge token supplied by the owner);
  an upload token must be added as the GitHub **`CODECOV_TOKEN`** secret
  (owner-side, same as `DOCKERHUB_TOKEN` for the release workflow).

## Decisions

1. **One new CI workflow** (`.github/workflows/ci.yml`) that runs on pushes to
   the default branch and on pull requests, with two jobs — `backend` and
   `frontend` — each uploading its own lcov to Codecov with a distinct flag
   (`backend` / `frontend`). Backend uses `make coverage`'s own script so CI
   enforces the same thresholds the repo already gates on; ubuntu-latest
   provides the Docker daemon the Postgres `testcontainers` tests need.
2. **Codecov should reflect the repo's *production-only* numbers**, not the
   inflated raw lcov (which counts test scaffolding). A tiny, isolated script
   [`scripts/lcov-production-only.sh`](../scripts/lcov-production-only.sh)
   filters a full lcov into a self-consistent production-only lcov
   (drops `/tests/`, `tests.rs`/`test.rs` files and every line from the first
   `#[cfg(test)]` marker; recomputes `LF`/`LH`). It is **not** part of
   `make coverage` (that stays byte-for-byte unchanged) — CI uses it only to
   build the upload artifact `backend/target/coverage/lcov.production.info`.
   The awk logic is validated locally against the committed-tree lcov artifact
   already present in `backend/target/coverage/lcov.info`.
3. **Frontend unit-test harness = Vitest** (`vitest@^4` +
   `@vitest/coverage-v8@^4` — the versions already used in-tree by the
   maplibre deps against Vite 8). Node `node` environment, coverage emitted as
   `text` + `lcov` into `frontend/coverage/lcov.info` (already ignored by the
   root `.gitignore`: `coverage/`). Vitest's v8 coverage provider always
   enumerates every `include`d file, so `coverage.include` is scoped to the
   pure-logic modules under test — React components are covered by Playwright
   e2e, not by this unit lcov, and would otherwise drag the report to ~0 %.
4. **Starter unit tests** are added for the pure-logic modules (no DOM /
   maplibre): `lib/format.ts`, `lib/geo.ts` (incl. `mapBounds` with a stubbed
   map), `lib/utils.ts`, `features/map/clusterStations.ts`,
   `features/stationDetail/resolution.ts` and the exported helpers of
   `features/stationDetail/timeframes.ts`. This makes the frontend Codecov
   flag report a real, growing baseline. No thresholds are enforced yet
   (`codecov.yml` is informational).
5. **Codecov config is non-blocking** initially (`codecov.yml`): `target: auto`
   with a small threshold, `patch` not failing PRs, and no PR-comment spam
   beyond the default summary. Coverage becomes a trend tracker first; strict
   gates can be switched on later.
6. **Docs** get the badge plus short notes (README, `agents.md`, CONTRIBUTING)
   so the coverage story is discoverable.

## Architect review

- **Approved.** The scope is correctly bounded to a coverage-reporting CI
  workflow plus the frontend unit-test harness Codecov needs a `frontend` flag
  to have data for.
- Confirmed the stack mismatch is intentional and documented: the backend is
  Rust (not Go) and the frontend had no unit-test runner, so Vitest is
  introduced.
- Confirmed **Vitest 4.x is the compatible major** for the installed
  **Vite 8.2.2**: the in-tree dependency graph (e.g. `maplibre-gl`) pins
  `vitest@^4.1.5` against `vite@^8.2.2`. Pin `vitest@^4.1.5` +
  `@vitest/coverage-v8@^4.1.5`.
- The production-only lcov filter (Decision 2) is the right call to keep
  Codecov aligned with the repo's `make coverage` gate numbers. Its awk logic
  mirrors [`scripts/coverage.sh`](../scripts/coverage.sh:61); it must be
  exercised against the committed-tree `backend/target/coverage/lcov.info`
  during implementation (shell execution is unavailable in architect mode).
- Keep `fail_ci_if_error: false` on the Codecov uploads so a fork PR lacking
  `CODECOV_TOKEN` cannot fail the workflow; add top-level
  `permissions: contents: read`.
- No changes to [`scripts/coverage.sh`](../scripts/coverage.sh) or the existing
  `make coverage` gate — the new filter is a separate, opt-in script used only
  by CI.

## Implementation steps

- [`codecov.yml`](../codecov.yml) (new) — `flags` (backend, frontend,
  `carryforward: true`), `coverage.status` informational, `comment`, and
  `ignore` for generated dirs (`backend/target`, `frontend/node_modules`,
  `frontend/dist`, `frontend/coverage`, `tiles`).
- [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) (new):
  - `backend` job — `actions/checkout@v7`, `dtolnay/rust-toolchain@stable`
    (`llvm-tools-preview`), `Swatinem/rust-cache@v2` (workspace `backend`),
    `taiki-e/install-action@cargo-llvm-cov`, `cargo fmt --check` +
    `cargo clippy --all-targets -- -D warnings` (fail fast), then
    `./scripts/coverage.sh` (full instrumented suite + thresholds), then
    `./scripts/lcov-production-only.sh backend/target/coverage/lcov.info >
    backend/target/coverage/lcov.production.info`, then
    `codecov/codecov-action@v5` uploading that file with `flags: backend` and
    `token: ${{ secrets.CODECOV_TOKEN }}`, `fail_ci_if_error: false`.
  - `frontend` job — `actions/setup-node@v4`
    (`node-version-file: frontend/.nvmrc`, npm cache keyed on the lockfile),
    `npm ci`, `npm run format:check` (the Prettier gate), then
    `npm run test:unit -- --coverage` (Vitest + lcov), then
    `codecov/codecov-action@v5` uploading `frontend/coverage/lcov.info` with
    `flags: frontend`.
- [`frontend/package.json`](../frontend/package.json) — add
  `devDependencies`: `vitest@^4.1.5`, `@vitest/coverage-v8@^4.1.5`; add
  scripts: `test:unit` (`vitest run`), `test:unit:watch` (`vitest`),
  `test:unit:coverage` (`vitest run --coverage`).
- [`frontend/vitest.config.ts`](../frontend/vitest.config.ts) (new) — `node`
  environment, `@` alias, test include `src/**/*.test.{ts,tsx}`, coverage
  provider `v8`, reporters `['text', 'lcov']`, reportsDirectory `coverage`,
  `coverage.include` scoped to the tested pure-logic modules.
- Starter tests (new, colocated under `frontend/src`):
  `lib/format.test.ts`, `lib/geo.test.ts`, `lib/utils.test.ts`,
  `features/map/clusterStations.test.ts`,
  `features/stationDetail/resolution.test.ts`,
  `features/stationDetail/timeframes.test.ts`.
- [`scripts/lcov-production-only.sh`](../scripts/lcov-production-only.sh)
  (new) — awk filter as described in Decision 2; validated on the existing
  `backend/target/coverage/lcov.info`.
- [`Makefile`](../Makefile) — add a `test-unit` target
  (`npm run test:unit --prefix frontend`) + `.PHONY` + help line, so the
  frontend unit suite is reachable the same way as the other gates.
- [`README.md`](../README.md) — add the Codecov badge to the top badge row.
- [`agents.md`](../agents.md) — extend the **Coverage** section: Codecov
  upload via CI (backend/frontend flags), the production-only upload artifact,
  and the new `make test-unit` command; keep the existing gate table intact
  (Codecov is informational for now).
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — one line noting frontend unit
  coverage (Vitest) is reported to Codecov alongside the Rust gate.

## Notes / non-goals

- Playwright e2e stays out of Codecov (browser e2e does not produce the
  source lcov this workflow tracks). Not wired into this CI yet.
- `cargo audit` runs in the local `make check`; it is intentionally not
  duplicated here (this workflow is about coverage reporting).
- The Codecov badge renders after the first upload lands on the default
  branch; until then it shows "unknown"/empty.
- The starter suite surfaced a latent bug in the (currently unused)
  `escapeHtml` helper of
  [`lib/format.ts`](../frontend/src/lib/format.ts): it prepended the literal
  character (`<` → `<lt;`) instead of the `&`-entity prefix. Corrected to the
  documented behaviour — the helper has no callers, so no behaviour changes.

## Definition of done

- [x] Plan reviewed by the architect
- [x] `.github/workflows/ci.yml` added with `backend` and `frontend` coverage jobs uploading to Codecov (flags `backend` / `frontend`)
- [x] `codecov.yml` added (informational, flags + ignore)
- [x] Frontend Vitest harness added (`package.json` deps/scripts, `vitest.config.ts`, Makefile `test-unit`)
- [x] Starter unit tests for the pure modules are green (`npm run test:unit` in `frontend/`) and produce `coverage/lcov.info`
- [x] `scripts/lcov-production-only.sh` added and validated against the existing `backend/target/coverage/lcov.info` (self-consistent output: no test files, recomputed `LF`/`LH` equal a direct recount)
- [x] Codecov badge added to `README.md` (exact URL requested by the owner)
- [x] `agents.md` and `CONTRIBUTING.md` updated
- [x] Local gates green: frontend `npm run test:unit` (50 tests), `npm run format:check`, `npm run build` (tsc), shell/awk syntax + self-consistency validation; plan `Status:` + checkboxes current
