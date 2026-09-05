# 103 — Disable scheduled jobs for Playwright + refresh e2e fixture from the current DB

## Context / problem

`make test-playwright` boots the real Docker Compose stack (nginx → backend BFF →
Postgres) seeded from the committed [`frontend/e2e/e2e-seed.sql`](frontend/e2e/e2e-seed.sql:1)
fixture. The run is documented as "fully offline", but that guarantee currently
rests on a fragile trick: the seed inserts pre-finished `jobs` rows so the
schedulers' startup `run_if_due()` never runs. There is no actual switch that
disables job scheduling, so any drift (fixture age, clock skew, a changed cron)
lets the backend start data-import jobs that reach out to the internet.

Additionally the committed fixture is stale:

- it only carries **3** data sources (Münster, Bonn, Hamburg) while the
  production [`config.toml`](config.toml:28) configures **6** (adding
  Eco-Counter, Hessen Mobil, Düsseldorf, Köln);
- its schema/refinery history lags the latest migrations (see the pending
  follow-up in [`ToDo.md`](ToDo.md:31) to regenerate from a V21 stack).

## Goals

1. Give the backend an explicit config switch to disable the scheduled
   background jobs, and set it in the Playwright e2e config so the e2e run can
   never start a data-import (or tiles / asset-cleanup) job.
2. Regenerate the e2e fixture **once** from the currently running stack's
   database (read-only), expanding e2e to all 6 production data sources, and
   reconcile the e2e specs that encode the old 3-source / 7-station assumptions.
   The development `postgres_data` / `minio_data` volumes and the dev database
   contents must not be deleted or modified.

## Changes

### 1. Backend: add `scheduled_jobs_enabled` config switch (default true)

- [`backend/src/core/domain/configuration/configuration.rs`](backend/src/core/domain/configuration/configuration.rs:19)
  - add a `scheduled_jobs_enabled: bool` field to `Configuration`,
  - initialize it to `true` in [`Configuration::new`](backend/src/core/domain/configuration/configuration.rs:39),
  - add `pub fn with_scheduled_jobs_enabled(mut self, enabled: bool) -> Self`,
  - add `pub fn scheduled_jobs_enabled(&self) -> bool`,
  - add unit tests for the default and the builder override (keeps core coverage
    ≥ 95%).
- [`backend/src/adapter/driven/configuration_toml_adapter.rs`](backend/src/adapter/driven/configuration_toml_adapter.rs:17)
  - add `#[serde(default = "default_scheduled_jobs_enabled")] scheduled_jobs_enabled: bool`
    to `ConfigurationDto` plus `fn default_scheduled_jobs_enabled() -> bool { true }`,
  - apply `.with_scheduled_jobs_enabled(dto.scheduled_jobs_enabled)` to the
    `Configuration::new(...)` result in `read_configuration`,
  - add tests: default `true` when omitted, `false` when set.
- [`backend/src/main.rs`](backend/src/main.rs:305)
  - wrap the three `tokio::spawn(job_scheduler::run_scheduler(...))` calls in
    `if configuration.scheduled_jobs_enabled() { ... }`.
- [`config.toml.example`](config.toml.example:6)
  - document the new key (`scheduled_jobs_enabled = true` with a comment noting
    it is default-true and can be disabled for tests). The dev
    [`config.toml`](config.toml:1) stays untouched; omitting the key keeps jobs
    enabled.

### 2. e2e orchestrator: disable jobs + configure all 6 data sources

- [`scripts/e2e-playwright.sh`](scripts/e2e-playwright.sh:85)
  - add `scheduled_jobs_enabled = false` to the temporary config,
  - replace the 3 `[[data_sources]]` blocks with the same 6 blocks as
    [`config.toml`](config.toml:28) (Münster, Bonn, Hamburg, Eco-Counter,
    Hessen Mobil, `Landeshauptstadt Düsseldorf | Dauerzählstellen Radverkehr`,
    `Stadt Köln`) so the startup sync keeps all seeded sources instead of
    cascade-deleting them,
  - update the header/heredoc comments ("all six data sources", "jobs disabled").
- [`frontend/e2e/docker-compose.e2e.yml`](frontend/e2e/docker-compose.e2e.yml:1)
  - update the comment that says "no provider import" to note jobs are disabled
    via config (no functional change).

### 3. Fixture regeneration (read-only, one-time)

- [`scripts/dump-e2e-fixture.sh`](scripts/dump-e2e-fixture.sh:74)
  - after the existing station-image unlink, add
    `UPDATE data_sources SET logo_asset_id = NULL, logo_sha256 = NULL;` so the
    e2e uses the bundled data-source SVG fallback (provider logo objects are not
    in the e2e MinIO bucket),
  - update the "pre-finished jobs" comment to note jobs are additionally
    disabled via `scheduled_jobs_enabled = false`.
- The script is already read-only w.r.t. the database (`docker compose exec -T db
  pg_dump` / `psql`); it only writes `frontend/e2e/e2e-seed.sql`. Do **not** run
  `docker compose down -v` and do **not** modify the dev volumes.

### 4. Reconcile e2e specs

- [`frontend/e2e/data-sources.spec.ts`](frontend/e2e/data-sources.spec.ts:13)
  - `toHaveCount(3)` → `toHaveCount(6)`,
  - assert visibility of the 4 added sources (Eco-Counter, Hessen Mobil,
    Düsseldorf, Köln),
  - update the file doc comment.
- Verify and, where needed, update the encoded station-count assumptions:
  - [`frontend/e2e/summary.spec.ts`](frontend/e2e/summary.spec.ts:151) — the
    Münster+Bonn bounds may now also include Köln/Düsseldorf stations without
    synthesized measurements; confirm the "too many data-streams" note still
    renders and fix the "7 stations" comment (tighten bounds if flaky),
  - [`frontend/e2e/sidebar.spec.ts`](frontend/e2e/sidebar.spec.ts:126) — confirm
    no new source stations land in the Münster view and that "only four Münster
    stations have synthesized data" still holds,
  - [`frontend/e2e/detail.spec.ts`](frontend/e2e/detail.spec.ts:151) — confirm
    the hardcoded Gasselstiege UUID is still present in the regenerated fixture;
    update it if the dev DB IDs changed.
- [`frontend/e2e/helpers.ts`](frontend/e2e/helpers.ts:54) — no change required
  (map/summary city coverage stays Münster/Bonn/Hamburg).

### 5. Docs

- [`agents.md`](agents.md:68) — update the Frontend e2e section and the
  `make test-playwright` row to state the run is jobs-disabled and covers all
  six data sources.
- [`README.md`](README.md:759) — same data-source wording update.
- [`plans/README.md`](plans/README.md:11) — register this plan.

## Execution order

1. Backend config switch + tests (`make test-rest` / `make test`).
2. e2e orchestrator + fixture-dump script changes.
3. Boot the stack if needed, run `scripts/dump-e2e-fixture.sh` to regenerate
   `frontend/e2e/e2e-seed.sql` (read-only against the running db).
4. Reconcile the specs against the regenerated fixture.
5. Docs + plan registry.
6. Gates: `make check`, `make test` (or `make test-rest`), `make coverage`,
   `make test-playwright`.

## Implementation notes (2026-09-05)

- **Source count correction:** the running `config.toml` / dev DB actually
  configures **seven** data sources (Münster, Bonn, Hamburg, Eco-Counter,
  Hessen Mobil, `Landeshauptstadt Düsseldorf | Dauerzählstellen Radverkehr`,
  Stadt Köln) at refinery **V21** — not the "6 at V21+" the plan estimated when
  written. All count references below therefore use **7** (the plan's own
  enumeration already listed all seven). The e2e config in
  [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh) mirrors
  [`config.toml`](../config.toml) exactly so the startup sync neither creates nor
  cascade-deletes seeded sources.
- [`scripts/dump-e2e-fixture.sh`](../scripts/dump-e2e-fixture.sh) was also out of
  sync with the committed seed (its synthesis list had dropped Gasselstiege while
  the seed + specs still relied on it); Gasselstiege (and the >5-stream comment)
  were restored so the regenerated fixture keeps 4 Münster stations with data.
- The fixture was regenerated read-only from the running stack
  (`scripts/dump-e2e-fixture.sh`): V21 schema + refinery history, all 7 sources,
  805 counting stations / 979 channels (with the `status` column), station images
  and per-source logos unlinked. Dev volumes/db untouched.
- The station UUIDs changed (data_source ids are deterministic UUIDv5, station ids
  are not): `detail.spec.ts` Gasselstiege → `04f14edd-…`, `flags.spec.ts`
  Gartenstraße → `2b410a8a-…`.
- `data-sources.spec.ts` now expects 7 source cards and asserts visibility of the
  four added sources. `sidebar.spec.ts` comments reconciled (the Münster bbox
  still contains only Münster's 23 stations). `summary.spec.ts`: its comment was
  updated (the summary bbox adds Köln/Eco-Counter stations **without** data, so
  the 7-data-stream note is unaffected) and `smallSummaryUrl` now scopes to one
  of the seeded data-bearing stations — the global `/api/bff/stations/search`
  spans all 7 sources, so the old "first positioned station" could be a data-less
  Hessen/Köln station (the summary then showed "No data for this period" and no
  key facts; this surfaced as the one e2e failure on the first run and is fixed).
- The Playwright orchestrator now writes `scheduled_jobs_enabled = false` to the
  temporary config; the e2e `docker-compose` comment and the V17-status comment in
  [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh) were refreshed.

## Definition of done

- [x] `scheduled_jobs_enabled` plumbed through config → `main.rs`; default true;
      e2e config sets `false`.
- [x] `frontend/e2e/e2e-seed.sql` regenerated from the current DB (schema +
      refinery history + all 7 data sources + stations/channels), dev volumes
      untouched.
- [x] `frontend/e2e/data-sources.spec.ts` expects 7 sources; other specs'
      station-count assumptions reconciled.
- [x] `make check` green.
- [x] `make test` / `make test-rest` green.
- [x] `make coverage` green (overall ≥ 80%, core ≥ 95%).
- [x] `make test-playwright` green with jobs disabled.
- [x] Docs (`agents.md`, `README.md`, ToDo.md, plan registry) updated.
