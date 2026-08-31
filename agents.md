# Agents

Conventions and required workflows for AI agents and contributors working in
this repository. **Read this file before making any change.**

## Workflow

1. **Write a plan first.** Before implementing anything, create or update a
   numbered plan document in [`plans/`](plans) — `plans/NN_<topic>_plan.md`,
   following the existing format — and register it in
   [`plans/README.md`](plans/README.md). This is **mandatory**: every change
   gets a plan file, no exceptions.
2. **Implement** the change, keeping it scoped to the plan.
3. **Run the gates** before finishing (see below) — all must pass.
4. **Update the docs** the change touches ([`README.md`](README.md),
   [`ToDo.md`](ToDo.md), and the plan file itself).
5. Do not risk wasting tokens for commands. Use tail / head when possible. The
   Make targets and [`scripts/`](scripts) are intentionally quiet: cargo runs
   with `--quiet`, and docker/npm build logs are redirected to a temp log that
   is only `tail`-ed on failure. When running a gate yourself, prefer `make
   check` / `make test-rest` over raw cargo, and pipe anything verbose through
   `tail -n 40` (or the script's own failure-only output) instead of dumping the
   full log.

## Required gates (run before finishing any change)

| Command | Purpose |
|---|---|
| `make check` | `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + frontend `prettier --check` + `cargo audit` (fails on any advisory; [`scripts/audit.sh`](scripts/audit.sh)) |
| `make test` | Full test suite (Postgres repository tests spin up a Docker test container) |
| `make test-rest` | REST endpoint tests only (in-memory mocks, no Docker required) |
| `make coverage` | **Coverage gate — fails when overall *production* line coverage is below `COVERAGE_THRESHOLD` (default 80%) or the core (`src/core/`) is below `CORE_COVERAGE_THRESHOLD` (default 95%)** |
| `make test-playwright` | **Frontend browser e2e — Playwright against the real Docker Compose stack seeded from a committed SQL fixture (Münster, Bonn, Hamburg; no provider import — see [Frontend e2e](#frontend-e2e-playwright) below)** |

### Coverage

`make coverage` runs [`scripts/coverage.sh`](scripts/coverage.sh), which executes
the whole test suite with LLVM instrumentation (`cargo-llvm-cov`) and **fails the
build when overall production line coverage drops below `COVERAGE_THRESHOLD`
(default 80%)** or when the **core** (`src/core/`, the domain + application
layer) drops below `CORE_COVERAGE_THRESHOLD` (default 95%). Both thresholds are
measured on **production code only**: lines inside `#[cfg(test)]` modules and
standalone test files are excluded, so test scaffolding can never inflate the
number. The core is pure hexagonal logic and is expected to be fully unit-tested
in isolation with in-memory mocks, so its bar is higher than the adapters'. New
code must keep coverage at or above the thresholds — prefer adding tests for new
behavior over lowering them.

Install the tooling once:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
cargo install cargo-audit
```

Notes:

- The full coverage run needs Docker (Postgres test container), like `make test`.
- The standard cargo-llvm-cov report is at `target/coverage/html/index.html`
  (open it with `make coverage-open`); it includes test scaffolding, so its
  totals differ from the **production-only** gate numbers printed in the
  terminal. The lcov data is at `target/coverage/lcov.info`.
- For a one-off run use `COVERAGE_THRESHOLD=<percent>` and/or
  `CORE_COVERAGE_THRESHOLD=<percent> make coverage` — never commit a lowered
  threshold.

### Frontend e2e (Playwright)

`make test-playwright` runs the browser e2e suite against the **real** Docker
Compose stack (nginx → backend BFF → Postgres) seeded from the committed
[`frontend/e2e/e2e-seed.sql`](frontend/e2e/e2e-seed.sql) fixture (schema + refinery
migration history + all counting stations/channels + synthesized recent
measurements + pre-finished jobs). The run is **fully offline**: no provider
import, no Protomaps/tile download, and the backend healthcheck is overridden to
`/health/live` so readiness never pings the providers. The specs live in
[`frontend/e2e/`](frontend/e2e) with the config in
[`frontend/playwright.config.ts`](frontend/playwright.config.ts); the
[`scripts/e2e-playwright.sh`](scripts/e2e-playwright.sh) orchestrator boots the
stack via the [`frontend/e2e/docker-compose.e2e.yml`](frontend/e2e/docker-compose.e2e.yml) override (seed
mount + healthcheck), waits for readiness and the seeded stations per city, runs
`npx playwright test`, then tears everything down (a pre-existing `config.toml`
is backed up and restored).

- **Run**: `make test-playwright` (needs Docker; first run also installs the
  Chromium browser via `npx playwright install chromium`, or run
  `make playwright-install` once). **Note**: the run is isolated from your
  development data — it uses dedicated `postgres_data_e2e`/`minio_data_e2e`
  volumes (see [`frontend/e2e/docker-compose.e2e.yml`](frontend/e2e/docker-compose.e2e.yml)) that are
  dropped and re-seeded from the fixture on every run, so the dev
  `postgres_data`/`minio_data` volumes are never cleared.
- **Regenerate the fixture**: [`scripts/dump-e2e-fixture.sh`](scripts/dump-e2e-fixture.sh)
  re-creates [`frontend/e2e/e2e-seed.sql`](frontend/e2e/e2e-seed.sql) from a running stack
  (e.g. after a schema change).
- **Requirements**: Docker Compose v2, Node.js/npm with the frontend deps
  installed (`npm ci` in [`frontend/`](frontend)).
- **Update**: add/change specs in [`frontend/e2e/`](frontend/e2e) and re-run
  `make test-playwright`. The map markers expose the station name via
  `alt`/`title` for locators.
- **Target**: point Playwright at another frontend with `FRONTEND_URL`
  (default `http://localhost:8081`).

## Definition of done

- [ ] Plan file in [`plans/`](plans) updated and registered in
      [`plans/README.md`](plans/README.md)
- [ ] `make check` green
- [ ] `make test` and/or `make test-rest` green
- [ ] `make coverage` green (coverage at/above the threshold)
- [ ] `make test-playwright` green when the change touches the frontend UI
- [ ] `README.md` / `ToDo.md` / plan docs updated as needed
