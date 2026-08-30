# 68 - Isolate PostgreSQL on an internal Docker network

Status: in progress

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

## Definition of done

- [ ] Postgres has no host port and is reachable only inside Docker.
- [ ] Backend reaches Postgres over the internal `db_network`.
- [ ] README updated; [`plans/README.md`](../plans/README.md:1) registration
      updated.
