# 74 - Isolate Playwright e2e from development data

Status: implemented

## Problem

[`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh:15) ran `docker compose
down -v` against the regular compose project, whose `db` and `minio` services use
the development `postgres_data` / `minio_data` volumes. `down -v` deletes those
named volumes, so every `make test-playwright` run wiped the developer's imported
stations and measurements (documented as intended in plan 71, but surprising and
destructive in practice).

## Goal

Make the Playwright e2e run fully isolated from development data: it keeps
getting a fresh, seeded database on every run, but the dev `postgres_data` /
`minio_data` volumes are never created, touched or deleted.

## Decisions

- Move the e2e `db` / `minio` services onto dedicated volumes
  (`postgres_data_e2e` / `minio_data_e2e`) via the existing
  [`frontend/e2e/docker-compose.e2e.yml`](../frontend/e2e/docker-compose.e2e.yml:1)
  override (relocated from the repo root by plan 75).
- Remove the dev volumes from the merged compose configuration with the Compose
  `!reset` merge tag, so `docker compose ... down -v` (used by the script before
  and after the run) can only ever drop the e2e volumes.
- Keep the script's container management as-is: it still stops/starts the
  compose stack (so it should not be run while the dev stack is up), but no
  volume is deleted. The dev stack can be brought back with `make run` and
  reuses the untouched volumes.
- The e2e volume is re-seeded from
  [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql:1) on its fresh
  volume exactly as before.

## Changes

### [`frontend/e2e/docker-compose.e2e.yml`](../frontend/e2e/docker-compose.e2e.yml:1)

- `db.volumes`: mount `postgres_data_e2e:/var/lib/postgresql` (instead of the
  dev `postgres_data`) + the seed bind-mount.
- `minio.volumes`: mount `minio_data_e2e:/data` (instead of `minio_data`).
- `volumes:`: declare `postgres_data_e2e` and `minio_data_e2e`, and reset the
  dev volumes out of the merged config:
  ```yaml
  postgres_data: !reset null
  minio_data: !reset null
  ```

### [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh:15)

- Update the header note: the run is isolated from development data; `down -v`
  only drops the e2e volumes.
- Clarify the teardown/clear messages ("e2e containers and volumes").

### [`agents.md`](../agents.md:85)

- Update the `make test-playwright` note to describe the dedicated e2e volumes
  and that dev data is never cleared.

## Verification

- `docker compose -f docker-compose.yml -f frontend/e2e/docker-compose.e2e.yml
  config` shows only `postgres_data_e2e` / `minio_data_e2e` volumes (dev volumes
  gone).
- `make test-playwright` green, and the dev `postgres_data` / `minio_data`
  volumes survive the run.

## Gates

- `make check` green.
- `make test-playwright` green.

## Definition of done

- [x] e2e uses dedicated `postgres_data_e2e`/`minio_data_e2e` volumes.
- [x] Dev `postgres_data`/`minio_data` are no longer referenced by the e2e
      compose config, so `down -v` cannot delete them.
- [x] Script + [`agents.md`](../agents.md:85) docs updated.
- [x] `make check` green.
