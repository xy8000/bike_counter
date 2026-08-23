Data Sources baseline

- [x] Configure data sources in config.toml as an array (nested name + provider type + key-value vars)
- [x] Extend Configuration domain with DataSourceConfiguration / DataProviderConfiguration (validation, unique names)
- [x] Parse nested data_sources in the TOML adapter
- [x] Define DataProvider trait (check_health, getAllCountingStations, getAllChannels, getMeasurements with paging)
- [x] MeasurementQuery always carries a Channel; from/to optional; max_batch_size per call
- [x] MeasurementBatch returns measurements + last_measurement_datetime + batch_size_limit_reached
- [x] ProviderHealthIndicator (per-data-source health component, name = "source/provider-type")
- [x] DataSource entity (id = UUID v5 hashed from name) + DataSourceRepository
- [x] Persist data_sources in the database; sync at startup (add new, remove stale)
- [x] ServiceHealthIndicator::name() returns String (dynamic provider names)
- [x] external_datasource_id on CountingStation and Channel; optional data_source_id FK (ON DELETE SET NULL)
- [x] Migration V2 (data_sources table, external_datasource_id columns, unique indexes)
- [x] DataProviderFactory (application trait + driven impl) builds providers from config
- [x] MuensterGithubAdapter implements DataProvider (config parsing + health check; import stubs)
- [x] StartupService in the application layer owns startup orchestration (main.rs = wiring only)
- [x] DataImportService capability (import paging) - no trigger yet (deferred feature)
- [x] REST: GET /api/v1/data-sources + root HATEOAS link + OpenAPI/Swagger update
- [ ] Implement GitHub-Data-Download (import only once)
- [ ] Load all Stations and print them on the console
- [ ] Stream Measurements to the console
- [ ] Expose import trigger (CLI/API) as a separate feature

Initial Setup of MuensterGithubAdapter

- [x] Setup initial domain
- [x] Setup TOML-Configuration-Adapter
- [x] Start TOML-Configuration-Adapter using real file
- [x] Write first TOML-Configuration-Adapter-Test
- [x] Setup Postgrest-DB-Adapter
- [x] Include Postgres-Testcontainer-Support using Docker-Compose
- [x] Expose a backend-api (REST) in order to recieve the data
- [] Implement GitHub-Data-Download (import only once)
- [] Load all Stations and print them on the console
- [] Stream Measuements to the console
- []

REST Driving Adapter (Axum + utoipa)

- [x] Add axum, tokio, utoipa, utoipa-swagger-ui, serde_json to Cargo.toml
- [x] Extend repository traits (find_all, find_by_counting_station_id, find_by_channel_id)
- [x] Define HATEOAS DTO models and links structure with utoipa schemas
- [x] Implement read-only (GET) endpoints under /api/v1 with flat URL hierarchy
- [x] Expose OpenAPI (utoipa) and Swagger-UI at /swagger-ui/
- [x] Wire up server startup in src/main.rs on 0.0.0.0:8080
- [x] Write integration tests for all endpoints (21 tests passing)


Generic Job Tracking, Cron Scheduler & Data Source Updater

- [x] Add cron crate + postgres with-serde_json-1 feature (Cargo.toml)
- [x] Migration V3: jobs table + data_sources.last_updated_at
- [x] Jobs domain module: Job entity + JobStatus (PENDING/RUNNING/FINISHED/FAILED) + JobRepository trait
- [x] PostgresJobRepository: lifetime_until TIMESTAMPTZ deadline, JSONB metadata (jsonb_set), atomic expire_running_jobs
- [x] DataSource.last_updated_at (DB-only) + update_last_updated_at repository method
- [x] Configuration: data_source_update_cron (default hourly, validated) + REQUIRED data_source_update_max_lifetime_seconds
- [x] DataImportService::update_data_source: stations -> channels -> paged measurements + progress callback + last_updated_at
- [x] DataSourceUpdateService job runner: expire stale RUNNING, skip-when-running, startup-if-never-succeeded, cron-tick, job lifecycle (incl. PENDING->FAILED), processed_measurements metadata, advance last_updated_at
- [x] Cron scheduler driver (tokio async loop) + wiring in main.rs
- [x] REST jobs endpoints (list + filters + get by id) + JobDto (lifetime_until + max_lifetime_exceeded) + OpenAPI + root jobs link
- [x] REST tests/mocks/fixtures for jobs + jobs integration tests
- [x] Application/config/postgres tests (lifecycle, decision, order, cron, TIMESTAMPTZ round-trip, expiry)
- [x] scripts/docker-compose-test.sh (end-to-end stack smoke test)
- [x] scripts/fmt-test.sh (cargo fmt --check + clippy -D warnings gate)
- [x] Docs (README, ToDo, plans/job_scheduler_plan.md)

Data Source Persistent State (plans/provider_state_storage_plan.md)

- [x] Migration V4: data_source_persistent_state table + index + provider-change revoke trigger
- [x] Core port PersistentStateStore (opaque KV, scoped per data source)
- [x] PersistentStateAccess handle + ScopedPersistentState (maps DomainError -> ProviderError::Storage)
- [x] DataProvider::attach_persistent_state (default no-op) — two-phase handover
- [x] PostgresPersistentStateRepository (get/set/delete/clear, upsert on the unique key)
- [x] PersistentStateService (resolves the data source, 404 unknown, delegates to the store)
- [x] StartupService: build provider -> upsert data source -> attach scoped handle
- [x] REST persistent_state endpoints (GET / PUT entry / DELETE entry / DELETE collection) through core
- [x] Münster adapter: holds the handle + cache_duration var (default 300)
- [x] Tests: repository + trigger + service + startup + adapter + REST persistent_state
