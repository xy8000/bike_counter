# Plan: REST through the core (refactor of the existing read endpoints)

Status: completed.

## Summary

Migrate every existing REST read endpoint so the handlers call thin core
application services instead of repository ports directly. DTO mapping stays in
the driving adapter (an adapter concern by design), and value-object construction
stays in the handlers — exactly the convention already used by the
`persistent_state` endpoints and [`PersistentStateService`](../src/core/application/persistent_state_service.rs).

## Scope

- Add five thin application services, one per resource:

  | Service | `list` signature | `find_by_id` signature | NotFound behaviour |
  |---|---|---|---|
  | `CountingStationService` | `list()` | `find_by_id(station_vo::Id) -> Result<CountingStation, DomainError>` | delegated (repo already errors `NotFound`) |
  | `ChannelService` | `list(Option<channel_vo::CountingStationId>)` | `find_by_id(channel_vo::Id) -> Result<Channel, DomainError>` | delegated |
  | `MeasurementService` | `list(Option<measurement_vo::ChannelId>)` | `find_by_id(measurement_vo::Id) -> Result<Measurement, DomainError>` | delegated |
  | `DataSourceService` | `list()` | `find_by_id(data_source_vo::Id) -> Result<DataSource, DomainError>` | maps `Option::None` to `DomainError::NotFound` |
  | `JobService` | `list(Option<String>, Option<JobStatus>)` | `find_by_id(Uuid) -> Result<Job, DomainError>` | maps `Option::None` to `DomainError::NotFound` |

  `CountingStationRepository`, `ChannelRepository` and `MeasurementRepository`
  already return a bare entity from `find_by_id` (they raise `NotFound` themselves),
  so those three services are pure delegation. `DataSourceRepository` and
  `JobRepository` return `Option`, so their services own the
  `ok_or(DomainError::NotFound(...))` mapping that currently lives in the handlers.

- Rewire [`AppState`](../src/adapter/driving/rest/handlers.rs:29) to hold the five
  services instead of the five repository ports. [`RestApiAdapter::new`](../src/adapter/driving/rest/mod.rs:38)
  changes its parameter list accordingly, and [`main.rs`](../src/main.rs) constructs
  the services and passes them in.
- Update every handler to call its service through the existing `blocking`
  (`spawn_blocking`) helper + `map_domain_error`.
- Job status string parsing stays in the handler and is passed to
  `JobService::list(job_type, status)`.
- Update the REST tests, fixtures, and mocks to the service-based `AppState`.
- The health endpoints are already service-based ([`HealthService`](../src/core/domain/health/service.rs))
  and the `persistent_state` endpoints already use [`PersistentStateService`](../src/core/application/persistent_state_service.rs);
  both stay as-is.

## Depends on

- [`provider_state_storage_plan.md`](provider_state_storage_plan.md) — already
  implemented. It introduced `PersistentStateService` and the service-through-core
  convention; this plan follows the same pattern for the remaining resources.

## Out of scope

- Provider persistent-state storage and its REST API (already in
  `provider_state_storage_plan.md`).
- The archive cache consumer and CSV parsing (`archive_cache_and_parsing_plan.md`).
- Moving REST DTO conversion into the core — DTOs remain an adapter concern.
- Any OpenAPI changes — no new routes or schemas are introduced; the existing
  `utoipa` annotations on the handlers remain valid.

## Step-by-Step Implementation

1. **Application services** — new files in [`src/core/application/`](../src/core/application)
   (register each in [`mod.rs`](../src/core/application/mod.rs)):
   - `counting_station_service.rs` — `CountingStationService`:
     `list()` delegates to `find_all`; `find_by_id(station_vo::Id)` delegates to
     `find_by_id` (repo already raises `NotFound`).
   - `channel_service.rs` — `ChannelService`:
     `list(Option<channel_vo::CountingStationId>)` branches between
     `find_by_counting_station_id` and `find_all`; `find_by_id(channel_vo::Id)`
     delegates.
   - `measurement_service.rs` — `MeasurementService`:
     `list(Option<measurement_vo::ChannelId>)` branches between
     `find_by_channel_id` and `find_all`; `find_by_id(measurement_vo::Id)` delegates.
   - `data_source_service.rs` — `DataSourceService`:
     `list()` delegates to `find_all`; `find_by_id(data_source_vo::Id)` maps
     `Option::None` to `DomainError::NotFound(id.0)`.
   - `job_service.rs` — `JobService`:
     `list(Option<String>, Option<JobStatus>)` delegates to
     `find_all(job_type.as_deref(), status)`; `find_by_id(Uuid)` maps `Option::None`
     to `DomainError::NotFound(id)`.

   Each service holds a single `Arc<dyn ...Repository + Send + Sync>` and exposes
   `new(repository)` — mirroring [`PersistentStateService`](../src/core/application/persistent_state_service.rs).

2. **Handlers** — [`src/adapter/driving/rest/handlers.rs`](../src/adapter/driving/rest/handlers.rs)
   - Replace the five repository fields in `AppState` with five service fields
     (`counting_station_service`, `channel_service`, `measurement_service`,
     `data_source_service`, `job_service`); keep `health_service` and
     `persistent_state_service`.
   - Rewrite each handler to clone the service and call it via `blocking(...)` +
     `map_domain_error`:
     - `list_counting_stations` / `get_counting_station_by_id`: call
       `CountingStationService`; the id value object is built in the handler.
     - `list_channels`: build `Option<channel_vo::CountingStationId>` from the query
       param, call `ChannelService::list`, keep the raw `Option<Uuid>` for
       `ChannelListDto::new`.
     - `get_channel_by_id`: call `ChannelService::find_by_id`.
     - `list_measurements`: build `Option<measurement_vo::ChannelId>`, call
       `MeasurementService::list`, keep the raw filter for `MeasurementListDto::new`.
     - `get_measurement_by_id`: call `MeasurementService::find_by_id`.
     - `list_data_sources` / `get_data_source_by_id`: call `DataSourceService`; drop
       the handler-side `.ok_or(DomainError::NotFound(id))`.
     - `list_jobs`: keep the `JobStatus::from_str` parsing; call
       `JobService::list(job_type, status)`.
     - `get_job_by_id`: call `JobService::find_by_id(id)`; drop the handler-side
       `.ok_or(DomainError::NotFound(id))`.
   - `blocking`, `map_domain_error`, and the `persistent_state` + health handlers
     are unchanged.

3. **Router wiring** — [`src/adapter/driving/rest/mod.rs`](../src/adapter/driving/rest/mod.rs)
   - Change `RestApiAdapter::new` to take the five services (plus the unchanged
     `health_service` and `persistent_state_service`) and populate the new `AppState`.
   - Routes, OpenAPI registration, and `run` are unchanged.

4. **Composition root** — [`src/main.rs`](../src/main.rs)
   - Keep the repository construction and the existing `DataImportService` /
     `DataSourceUpdateService` wiring (they keep using the repositories directly).
   - Construct `CountingStationService`, `ChannelService`, `MeasurementService`,
     `DataSourceService`, and `JobService` from the repository `Arc`s, then pass the
     services into `RestApiAdapter::new`.

5. **REST tests** — [`src/adapter/driving/rest/tests/`](../src/adapter/driving/rest/tests)
   - [`mocks.rs`](../src/adapter/driving/rest/tests/mocks.rs): keep the in-memory
     repositories (services still depend on the repository ports); add sample service
     constructors next to `sample_persistent_state_service()` —
     `sample_counting_station_service()`, `sample_channel_service()`,
     `sample_measurement_service()`, `sample_data_source_service()`, and
     `sample_job_service()` — each wrapping the corresponding mock repository in an
     `Arc`.
   - [`mod.rs`](../src/adapter/driving/rest/tests/mod.rs): update `TestApp` to pass
     services (not repositories) to `RestApiAdapter::new`; the custom-repository
     constructors (`with_jobs`, `with_repositories`) wrap the provided mock in a
     service before handing it off.
   - Assertions in the per-resource test modules stay unchanged; the endpoints must
     behave identically.

6. **Service unit tests**
   - Follow the [`persistent_state_service.rs`](../src/core/application/persistent_state_service.rs)
     pattern with local in-memory repositories per service `#[cfg(test)]` module:
     - `CountingStationService`: `list` returns all; `find_by_id` known/unknown.
     - `ChannelService`: `list` with and without the station filter; `find_by_id`.
     - `MeasurementService`: `list` with and without the channel filter; `find_by_id`.
     - `DataSourceService`: `list`; `find_by_id` known; `find_by_id` unknown maps to
       `NotFound` (this is the moved handler logic).
     - `JobService`: `list` with `job_type`/`status` filters; `find_by_id` known;
       `find_by_id` unknown maps to `NotFound`.

7. **Docs & tracking**
   - [`ToDo.md`](../ToDo.md): check off the items added for this plan.
   - [`plans/README.md`](../plans/README.md): keep the plan listed as the next one.

## Verification (via the Makefile)

```bash
make check       # cargo fmt --check + cargo clippy --all-targets -- -D warnings
make test        # full suite (Postgres tests use the Docker test container)
make test-rest   # REST endpoint tests against in-memory mocks
make test-e2e    # end-to-end smoke test against the docker-compose stack
```

A clean `make check` and `make test` are the acceptance gate. The existing REST
endpoints are pure refactors, so `make test-rest` must show the same behaviour as
before (same status codes and payloads).
