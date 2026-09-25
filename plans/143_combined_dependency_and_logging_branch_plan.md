# 143 - Combined dependency-bump + logging branch

Status: implemented

## Problem

Two independent branches diverged from `main` (`022a10c`) and must be combined
into a single branch:

- `chore/frontend-dependency-bump` (`5c90a88`) — frontend dependency bump plus
  the backend dependency bump and refreshed Docker base-image digests
  (plans [141](141_frontend_dependency_upgrade_plan.md) and
  [142](142_backend_dependency_and_docker_image_bump_plan.md)).
- `feat/logging-and-muenster-fetch-fix` (`ecd85c5`) — timestamped tracing logs,
  per-source failure isolation and the Münster archive-age fix
  (plan [139](139_timestamped_logging_and_update_semantics_plan.md)).

Both edit [`backend/Cargo.toml`](../backend/Cargo.toml:1) and
[`backend/Cargo.lock`](../backend/Cargo.lock:1), so a combined branch has to
reconcile the manifests (the dependency bump moved `utoipa` to 6.0.0 and
`arrow`/`parquet` to 60, while the logging commit adds `tracing` and still
referenced `utoipa` 5.5.0).

## Changes

1. Created branch `chore/combined-deps-and-logging` from
   `chore/frontend-dependency-bump` (`5c90a88`).
2. Cherry-picked `ecd85c5` onto it (new commit `763e4ad`). Git auto-merged
   [`backend/Cargo.toml`](../backend/Cargo.toml:1) and
   [`backend/Cargo.lock`](../backend/Cargo.lock:1) with **no conflicts**; the
   result was verified to be the union of both change sets:
   `arrow`/`parquet` 60.0.0, `utoipa` 6.0.0, `utoipa-swagger-ui` 10.0.1,
   `testcontainers` 0.27.3, `toml` `1.1.6` **plus** `tracing` 0.1.41 and
   `tracing-subscriber` 0.3.20.
3. No manual conflict resolution and no further lock churn were required.

## Verification

| Command | Result |
| --- | --- |
| `cargo check --all-targets` | green (lock already consistent) |
| `make check` | green (fmt, clippy, Prettier, audit) |
| `make test-rest` | 128 passed |
| `make test` | 771 passed (4 more than the dependency-only branch, from the logging commit's tests) |

## Definition of done

- [x] Combined branch `chore/combined-deps-and-logging` created
- [x] `ecd85c5` cherry-picked with the manifests reconciled
- [x] `make check` green
- [x] `make test-rest` green
- [x] `make test` green
- [x] plan file status/checklist kept current
