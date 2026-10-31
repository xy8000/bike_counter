# 170 - Async Database Driver Evaluation and Migration Plan

Status: proposed (evaluation; no driver migration in plan 164)

## Context

Plan [`164`](plans/164_f1_f5_hardening_plan.md:1) addressed finding F1
(synchronous `postgres` driver behind an async HTTP layer) with **capacity
controls only**: a configurable pool
([`create_pool_with`](backend/src/adapter/driven/postgres/pool.rs:41)), a server
side `statement_timeout`, a global `ConcurrencyLimitLayer` at the router and a
single shared tokio runtime for both storage adapters
([`S3AssetStorage::with_runtime`](backend/src/adapter/driven/s3_asset_storage.rs:59)).

That is a mitigation, not a fix. Every handler still funnels work through
[`blocking()`](backend/src/adapter/driving/rest/handlers/mod.rs:129) into
`spawn_blocking`, so a request occupies a blocking-pool thread while it waits for
one of `database_pool_max_size` connections. Plan 164 therefore deferred the real
migration to this plan, to be written **after** the read-path shape was fixed
(workstream D: set-based channel loading, 8 aggregate queries per timezone,
batched asset lookups).

## Goal

Decide, with a written comparison and a load test, whether to migrate the
persistence adapters to an async driver, and if so in which order and behind
which safety net.

## Questions to answer

1. **Port shape.** `MeasurementRepository` has 24 methods
   ([`repository_port.rs`](backend/src/core/domain/measurements/repository_port.rs:154)).
   Is the smaller change `async fn` in traits (AFIT, Rust 1.75+) with
   `Box<dyn>`/`async_trait` at the boundaries, or keeping the synchronous ports
   and moving only the *adapter* internals onto `tokio-postgres` driven through
   `spawn_blocking`? The latter keeps every core service and test double
   untouched but does not remove the blocking-thread hop.
2. **Driver choice.** `tokio-postgres` + `deadpool-postgres` versus `sqlx`:
   - refinery compatibility (the migrations run today inside
     [`create_pool`](backend/src/adapter/driven/postgres/pool.rs:41)),
   - prepared-statement reuse and typed row mapping versus the current
     `row.get("column")` calls,
   - `ANY($1::uuid[])` batch queries and `jsonb` handling used by the new read
     paths,
   - compile-time vs. runtime query checking (sqlx macros need a live database in
     CI unless offline mode is used).
3. **Cancellation semantics.** A tower-http timeout today only bounds the
   response; the blocking query keeps running. With an async driver, dropping the
   future should cancel the query — measure that assumption per driver.
4. **Connection budget.** Does an async pool change the relationship between
   `database_pool_max_size`, `http_max_concurrent_requests` and Postgres
   `max_connections`? Document the new model in the README.

## Migration and rollback strategy

- One repository at a time, starting with `PostgresMeasurementRepository` (the
  hot path), behind a feature flag or a trait-object switch in the composition
  root so the old adapter stays available.
- Keep the 24-method `MeasurementRepository` port unchanged during the first
  slice: implement it with async internals only if the port allows it, otherwise
  split the port first (F7) and migrate the read port.
- Every slice must be deployable on its own: migrations are unchanged (no schema
  change), so rollback is a redeploy of the previous image.
- Keep `blocking()` available until the last repository is migrated, so a mixed
  state is safe.

## Load test and acceptance criteria

- A repeatable load test driving the map pan pattern (three BFF calls per
  viewport change: stations, sidebar, sidebar stats) against a seeded database.
- Assert **p95 latency** for the triple request under the documented concurrency
  limit, and that latency no longer degrades to multiples of the pool acquisition
  timeout when the concurrency limit is reached.
- Compare against the plan 164 baseline (blocking pool + concurrency limit) and
  record both numbers in this plan.

## Out of scope

- Changing the analytics result shape or the BFF payloads.
- Partitioning `measurements` (F11) or replacing the process-local rollup
  readiness flag.

## Definition of done

- [ ] Written comparison of the port shapes (AFIT vs. blocking adapter) with a
      recommendation
- [ ] Written comparison of `tokio-postgres`/`deadpool-postgres` vs. `sqlx`
      against the four criteria above
- [ ] Cancellation semantics measured and documented
- [ ] Migration/rollback plan per repository, starting with measurements, behind
      a flag
- [ ] Load test committed and the p95 budget asserted in CI or in a documented
      manual run
- [ ] `README.md` capacity model updated for the new connection model
