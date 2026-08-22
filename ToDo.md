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


I now have driven controllers for accessing the database. Now i would like to create a driving adapter that is exposing a REST-Ful (including HATEOAS-Links) API. The API should be READ ONLY (GET). Start with /api/v1 and then have a flat hierarchy (do not chain IDs in the URL). Also should be exposed by Swagger-UI. Write an adapter (driven) that handles REST-Calls. Is there a way to automatically generate a swagger? if yes, do so. Otherwise please create another adapter for swagger (you can use an openSource alternative instead swagger as well)