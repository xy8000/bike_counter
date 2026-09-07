# 69 - Isolate PostgreSQL on an internal Docker network

Status: implemented

## Problem

PostgreSQL currently sits on the default Compose network and publishes
`5432:5432` to the host, so it is reachable from outside the stack. MinIO is
already isolated on a private `asset_network` (`internal: true`, no host port);
PostgreSQL should be isolated the same way.

## Scope

Infrastructure-only ([`docker-compose.yml`](../docker-compose.yml:1) + docs). No
backend, frontend or data-model changes.

## Decisions

- Fully isolate (confirmed with the user): remove the `5432:5432` host port so
  Postgres is reachable only from within Docker. Local access uses
  `docker compose exec db psql -U postgres -d bike_counter`.
- Add a dedicated `db_network` with `internal: true`, mirroring `asset_network`.
- Attach `db` to `db_network` only; attach `backend` to `default` +
  `asset_network` + `db_network` (the frontend still reaches the backend over
  the default network, and the backend still needs egress for data-source
  imports and the tile build).
- Keep the existing `db` healthcheck and the backend `depends_on` condition.

## Changes

### [`docker-compose.yml`](../docker-compose.yml:1)

- `db`:
  - drop the `ports: ["5432:5432"]` block,
  - add `networks: [db_network]`.
- `backend`:
  - extend `networks` to `default`, `asset_network`, `db_network`.
- `networks`:
  - add `db_network: internal: true` with a comment mirroring `asset_network`
    ("only the backend reaches Postgres; no host port published").

### [`README.md`](../README.md:394)

- Update the "Run with Docker Compose" bullet list to describe the isolated
  `db_network` (no host port; `docker compose exec db psql ...` for local access),
  mirroring the existing MinIO bullet.

## Verification

- `docker compose config` valid (no port, networks resolve).
- `make test-e2e` green — [`docker-compose-test.sh`](../scripts/docker-compose-test.sh:183)
  already uses `docker compose exec -T db psql ...`, which works without a host
  port; the backend reaches `db` over `db_network`.
- `make test-playwright` green (stack boots and imports normally).

## Gates

- `docker compose config` green.
- `make test-e2e` green.
- `make test-playwright` green (infra change touches the compose stack).

## Implementation notes

- [`docker-compose.yml`](../docker-compose.yml:1): `db` now uses
  `postgres:18-alpine` on `db_network` (`internal: true`) with **no** `ports`
  block; `backend` is attached to `default`, `asset_network` and `db_network`;
  the db healthcheck is unchanged.
- **Postgres 18 volume caveat**: PG 18 stores data under
  `/var/lib/postgresql` (major-version-specific subdirectory), so the volume
  mount changed from `/var/lib/postgresql/data`. A pre-existing volume created by
  an older major must be re-created (`docker compose down -v`) — the data is not
  migrated in place. (This is also part of the plan-68 image bump.)
- **Data-loss caveat (apply on a running instance!)**: re-creating the
  `postgres_data` volume wipes *all* previously imported counting-station data
  (stations, channels, measurements) for every data source. The data is not
  recoverable from the volume, but it **is** re-importable from the public
  provider APIs (Münster GitHub archive, Bonn OpenData, Hamburg SensorThings):
  restore the full three-source `config.toml` (all `[[data_sources]]` from
  [`config.toml.example`](../config.toml.example:38)) and let the hourly
  `data_source_update` job (plus the startup overdue run) backfill it. The
  Hamburg backfill is large (5-min history) and the upstream API intermittently
  throttles it — the retry logic (plan 68) rides through those transient
  `EAI_AGAIN` errors, so leave the stack running and let the job complete.
- **Recovery outcome (2026-08-30)**: after restoring the three-source
  `config.toml` and fixing the plan-68 Hamburg regressions, all **215 counting
  stations** are visible again (Bonn 17, Hamburg 175, Münster 23) and the
  providers report healthy. Münster (8.98M) and Bonn (332k) measurements are
  fully re-imported; Hamburg's 5-min history is backfilling progressively
  (900k+ and climbing) across the hourly job.
- Verified: `docker compose config` valid, `make test-e2e` green (the smoke test
  reaches `db` via `docker compose exec -T db psql`, no host port), and
  `make test-playwright` (23/23) green.
- Local access:
  `docker compose exec db psql -U postgres -d bike_counter`.

## Definition of done

- [x] Postgres has no host port and is reachable only inside Docker.
- [x] Backend reaches Postgres over the internal `db_network`.
- [x] README updated.
