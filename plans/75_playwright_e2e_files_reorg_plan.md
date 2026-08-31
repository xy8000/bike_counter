# 75 - Move e2e files into frontend/e2e/

Status: implemented

## Problem

The Playwright e2e material was split across the repo root and `scripts/`: the
compose override lived at `docker-compose.e2e.yml` and the SQL fixture at
`scripts/e2e-seed.sql`, while the specs/helpers already live in
[`frontend/e2e/`](../frontend/e2e). For the user, all e2e-specific assets should
sit together under `frontend/e2e/`; only the run orchestrator stays in
[`scripts/`](../scripts).

## Goal

Move the e2e compose override and the SQL fixture into `frontend/e2e/`, and
update every reference (scripts, docs, plans) so `make test-playwright` keeps
working unchanged.

## Decisions

- `docker-compose.e2e.yml` → [`frontend/e2e/docker-compose.e2e.yml`](../frontend/e2e/docker-compose.e2e.yml:1).
- `scripts/e2e-seed.sql` → [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql:1).
- The scripts stay in [`scripts/`](../scripts): `e2e-playwright.sh` (orchestrator)
  and `dump-e2e-fixture.sh` (fixture regeneration).
- Compose bind-mount paths resolve against the project directory (the first
  compose file's folder = repo root), not the override's own folder, so the
  seed mount in the override is `./frontend/e2e/e2e-seed.sql`.

## Changes

### [`frontend/e2e/docker-compose.e2e.yml`](../frontend/e2e/docker-compose.e2e.yml:1) (moved from repo root)

- Same content as before (e2e volumes + seed mount + `/health/live` override).
- Seed bind-mount now `./frontend/e2e/e2e-seed.sql` (project-root-relative), with
  a comment explaining the resolution rule.
- Header comments updated to the new paths.

### [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql:1) (moved from scripts/)

- Content unchanged; header comment updated to reference
  `frontend/e2e/docker-compose.e2e.yml`.

### [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh:34)

- `COMPOSE_OVERRIDE` now points to
  `${PROJECT_ROOT}/frontend/e2e/docker-compose.e2e.yml`.
- Header + inline comments updated to the new paths.

### [`scripts/dump-e2e-fixture.sh`](../scripts/dump-e2e-fixture.sh:27)

- `OUT` now points to `${PROJECT_ROOT}/frontend/e2e/e2e-seed.sql`.
- Header/comments updated.

### Docs

- [`agents.md`](../agents.md:72), [`README.md`](../README.md:687),
  [`frontend/e2e/helpers.ts`](../frontend/e2e/helpers.ts:49),
  [`frontend/e2e/cities.spec.ts`](../frontend/e2e/cities.spec.ts:5) and the
  recent plans (73/74) updated to the new paths.

## Verification

- `docker compose -f docker-compose.yml -f frontend/e2e/docker-compose.e2e.yml
  config` shows the seed bind-mount resolving to
  `…/frontend/e2e/e2e-seed.sql` and only the e2e volumes.
- `make test-playwright` green.

## Gates

- `make check` green.
- `make test-playwright` green.

## Definition of done

- [ ] Compose override + SQL fixture live in [`frontend/e2e/`](../frontend/e2e).
- [ ] All references (scripts, agents.md, README, plans) point at the new paths.
- [ ] Merged compose config resolves the seed from `frontend/e2e/e2e-seed.sql`.
- [ ] [`plans/README.md`](../plans/README.md:1) updated.
- [ ] `make check` green.
