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
- [x] Migration V3: jobs table + data_sources.imported_until (added as last_updated_at, renamed in V7)
- [x] Jobs domain module: Job entity + JobStatus (PENDING/RUNNING/FINISHED/FAILED) + JobRepository trait
- [x] PostgresJobRepository: lifetime_until TIMESTAMPTZ deadline, JSONB metadata (jsonb_set), atomic expire_running_jobs
- [x] DataSource.imported_until (DB-only, renamed in V7) + update_imported_until / clear_imported_until repository methods
- [x] Configuration: data_source_update_cron (default hourly, validated) + REQUIRED data_source_update_max_lifetime_seconds
- [x] DataImportService::update_data_source: stations -> channels -> paged measurements + progress callback + imported_until
- [x] DataSourceUpdateService job runner: expire stale RUNNING, skip-when-running, startup-if-never-succeeded, cron-tick, job lifecycle (incl. PENDING->FAILED), processed_measurements + added_measurements metadata, advance imported_until
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

REST through the core (plans/rest_through_core_plan.md)

- [x] CountingStationService (list / find_by_id) + register in src/core/application/mod.rs
- [x] ChannelService (list with station filter / find_by_id) + register
- [x] MeasurementService (list with channel filter / find_by_id) + register
- [x] DataSourceService (list / find_by_id with Option -> NotFound mapping) + register
- [x] JobService (list with job_type + status filters / find_by_id with Option -> NotFound mapping) + register
- [x] Rewire AppState to hold the five services; update handlers to call services via blocking + map_domain_error
- [x] Update RestApiAdapter::new signature and main.rs wiring
- [x] Update REST tests / fixtures / mocks to the service-based AppState (sample service constructors)
- [x] Add service unit tests (local in-memory repos; NotFound mapping + filter branching)
- [x] make check + make test + make test-rest green

Archive cache + CSV parsing (plans/archive_cache_and_parsing_plan.md)

- [x] Add dependencies: ureq, zip, csv, chrono-tz
- [x] Record-based DataProvider interface (core owns identity): CountingStationRecord / ChannelRecord / MeasurementRecord + MeasurementBatch records
- [x] DataImportService maps records to entities (UUID generation, channel↔station linking, measurements attach channel id)
- [x] site_min.json parsing (stations + channels; station-aggregate entry skipped)
- [x] Monthly CSV parsing (Europe/Berlin → UTC, integer values, `-status` + aggregate columns skipped, empty cells skipped)
- [x] Four-tier archive cache over PersistentStateAccess (fresh-extract reuse / re-extract stale ZIP / HEAD change detection / download)
- [x] Münster adapter data-serving methods (stations, channels, paged measurements with exclusive `from`)
- [x] Unit tests: parsers, timezone DST, cache tiers (fake fetcher), adapter end-to-end
- [x] Existing DataProvider mocks and tests updated to the record interface
- [x] Docs (README, ToDo, plans/archive_cache_and_parsing_plan.md)

Overdue-run for the data-source update job (plans/startup_overdue_update_plan.md)

- [x] Remove the `startup` flag; `run_if_due()` applies one always-on rule: run if never succeeded or the last successful run is overdue
- [x] `is_overdue` helper: first cron trigger after the last `finished_at` has passed (uses the validated cron schedule)
- [x] Job logs include name + id: `Data source update job {name} ({id}) started` / `finished` / `failed`
- [x] `run_scheduler` simplified: same `run_if_due()` for the immediate startup call and every cron tick
- [x] Tests: `runs_when_last_run_is_overdue` + updated call sites (`finished_job_at` helper)
- [x] make check + make test green (147 tests)

Import time-batching + API pagination & filters (plans/import_timeframe_and_api_pagination_plan.md)

- [x] Provider var `max_measurement_timeframe_hours` (default 168) parsed in the Münster adapter + config.toml(.example)
- [x] Channel→CSV index derived from `site_min.json` + station directory listing (no CSV header reads; `read_csv_channel_ids` removed)
- [x] Windowed file selection in `get_measurements` (only overlapping months parsed) + separate `timeframe_limit_reached` flag
- [x] `MeasurementBatch.timeframe_limit_reached` + import loop pages while `batch_size_limit_reached || timeframe_limit_reached`
- [x] Migration V5: dedupe on `(channel_id, timestamp)` + `UNIQUE` natural key
- [x] Idempotent `save`/`save_batch` (`INSERT ... ON CONFLICT (channel_id, timestamp) DO NOTHING`)
- [x] Measurements offset/limit pagination (repository `find_page`, service `list`, DTO links + `offset`/`limit`; handler defaults `limit` to 5000 with no upper bound)
- [x] Name filters for counting-stations and channels (`find_filtered` + ILIKE + DTO/handler + query params)
- [x] Tests: adapter windowed/gap/config, import-loop timeframe paging, REST pagination + name filters, Postgres pagination + natural-key idempotency
- [x] Docs (README, ToDo, plans/import_timeframe_and_api_pagination_plan.md)
- [x] make check + make test green

Raw measurements export (plans/17_measurement_counting_station_filter_plan.md)

- [x] `GET /api/v1/measurements/raw`: bare JSON array of plain measurement objects (no HATEOAS links / no pagination envelope), same `channel_id`/`offset`/`limit` query parameters, handled in the driving adapter only
- [x] Fix `ProviderMessageDto` self link (no single-message endpoint; points at the messages collection)
- [x] Tests (raw endpoint, `RawMeasurementDto` mapping, provider-message self link, OpenAPI path) + docs
- [x] make check + make test + make test-rest + make coverage green

Unique counting-station and channel names (plans/19_counting_station_channel_name_uniqueness_plan.md)

- [x] Adapter dedup: `parse_site_index` appends the external id to duplicate channel names (per station) and duplicate station names (per archive)
- [x] Migration V8: repair existing duplicate rows (append external id / fallback to row id), then `UNIQUE (data_source_id, name)` partial + `UNIQUE (counting_station_id, name)`
- [x] DB-enforced `data_source_id`: migration V9 backfills pre-linking rows (single data source), adds `NOT NULL`, and switches the FK chain to `ON DELETE CASCADE` (no core changes)
- [x] Counting-station Swagger schema exposes the required `data_source_id` and an always-present `data_source` HATEOAS link
- [x] Tests (Münster parser dedup, counting-station DTO field/link, REST endpoint `data_source_id`) + docs
- [x] make check + make test + make test-rest + make coverage green

Frontend + BFF module + monorepo restructure (plans/20_frontend_bff_monorepo_plan.md)

- [x] Monorepo layout: backend files moved to `backend/` (Cargo.toml, src/, migrations/, config.toml.example, Dockerfile, docker/); Makefile, docker-compose.yml, scripts/, plans/ and docs stay at the root
- [x] Docker Compose ramps up the whole stack: `db` + `backend` (renamed from `app`, builds `./backend`) + `frontend` (nginx on 8081, reverse-proxies `/api` to backend:8080)
- [x] BFF Rust module `backend/src/adapter/driving/bff/` exposing `GET /api/bff/hello` -> `{"message": "Hello from BFF"}`, wired into the router and the same Swagger doc under a new `BFF API` tag
- [x] BFF REST tests (hello returns 200 + message; OpenAPI contains `/api/bff/hello` and the `BFF API` tag)
- [x] React (Vite + TypeScript) frontend in `frontend/`: static build served by nginx, fetches `/api/bff/hello` and renders "Hello World" + the BFF message
- [x] `frontend/Dockerfile` (node build -> nginx) + `nginx.conf` (SPA + `/api` reverse proxy) + `package-lock.json`
- [x] Makefile + scripts (`fmt-test.sh`, `coverage.sh`, `docker-compose-test.sh`) updated for the `backend/` crate; `docker-compose-test.sh` now also asserts `/api/bff/hello` and the frontend page
- [x] Docs (README, ToDo, plans/20_frontend_bff_monorepo_plan.md)
- [ ] make check + make test + make test-rest + make coverage + make test-e2e green

Tooling upgrade: npm/Node + deps + slimmer Docker images (plans/21_tooling_upgrade_plan.md)

- [x] Bump frontend deps to latest stable majors (React 19.2, Vite 7.3, TypeScript 5.9, @vitejs/plugin-react 5.2, @types/react 19) + `engines` + `frontend/.nvmrc` (Node 24 LTS); regenerate `package-lock.json`
- [x] `frontend/Dockerfile`: `node:24-alpine` build (latest stable npm) -> pinned `nginx:1.31-alpine` runtime
- [x] `backend/Dockerfile`: `rust:1-alpine` (musl static) build -> `alpine:3.24` runtime, keeping `curl` + curl HEALTHCHECK (image ~39 MB vs ~90 MB Debian-based)
- [x] Scripts verified backend-only: `coverage.sh` + `fmt-test.sh` already `cd backend`; no frontend coverage/format script added
- [x] Gates green: `make check`, `make test` (221), `make test-rest` (66), `make coverage` (overall 82.10%, core 96.40%), `make test-e2e`, `make frontend-build`

Map view + counting-station GPS coordinates (plans/22_map_view_gps_coordinates_plan.md)

- [x] Hardcoded Münster station metadata (external id -> name/lat/lng) overlay in `get_all_counting_stations`; unlisted stations have "not provided" coordinates
- [x] Optional `GeoCoordinates` value object on `CountingStation` + optional `latitude`/`longitude` on `CountingStationRecord`
- [x] Migration V10: nullable `latitude`/`longitude` columns on `counting_stations`
- [x] Repository: read/write coordinates + new `update` method (upsert of name/description/coordinates)
- [x] Import `sync_counting_stations` upserts stations by external id (updates name/description/coordinates on every sync, inserts new); resync test
- [x] `CountingStationService::update_coordinates` + `PATCH /api/v1/counting-stations/{id}` (optional/nullable lat/lng) + OpenAPI/Swagger
- [x] REST counting-station DTO exposes `latitude`/`longitude`
- [x] Frontend: Leaflet map view (`react-leaflet`), centered on Münster, one marker per station with coordinates (name popup), loading/error states
- [x] Gates green: `make check`, `make test` (235), `make test-rest`, `make coverage` (overall 84.67%, core 96.56%), `make frontend-build`

Visible stations BFF + config consolidation + frontend header/list (plans/23_visible_stations_bff_and_config_plan.md)

- [x] Remove the placeholder `GET /api/bff/hello` (handler, DTO, route, Swagger path/schema) while keeping the `BFF API` Swagger tag
- [x] `GET /api/bff/stations` (optional bounding box -> visible or all stations) + `GET /api/bff/stations/summary` (header aggregate), computed on the fly by a new core `StationSummaryService` (`GeoBounds` + `find_within_bounds` + `sum_since`)
- [x] Frontend: Komoot-style header + left sidebar of visible stations + modal search dialog with "find on map"; map and list fed by `/api/bff/stations`, header aggregate by `/api/bff/stations/summary`
- [x] Config consolidated to the root `config.toml` (backend TOMLs removed; docker compose mounts `./config.toml` into backend + frontend; root `config.toml.example` template)
- [x] `[frontend] log_level` in the TOML wires nginx `error_log` via an entrypoint script + nginx template
- [x] Tests: BFF endpoint tests, `StationSummaryService` unit tests, Postgres `find_within_bounds`/`sum_since` tests; `docker-compose-test.sh` updated
- [ ] Gates green: `make check`, `make test`, `make test-rest`, `make coverage`, `make frontend-build`, `make test-e2e`

Visible-stations BFF refactor (plans/24_visible_stations_bff_refactor_plan.md)

- [x] `StationSummary` reuses the `CountingStation` entity; the BFF DTO mapping goes through `CountingStationDto` (same serialized JSON fields)
- [x] `MeasurementRepository::sum_since(&[ChannelId], since)` -> scalar `sum(from, to, channel_id?)`; Postgres impl + test renamed; every in-memory mock updated; `StationSummaryService` sums per station channel
- [x] `GeoBounds`/`find_within_bounds` removed from counting stations; new core `station_summary::bounds::GeoBounds` (`is_valid` + `contains`); the service filters stations in memory via `find_filtered(None)`
- [x] `StationSummaryAggregate` split into its own `station_summary/aggregate.rs` (global summary)
- [x] Service port + BFF handlers take an explicit `(from, to)` window (`to = Utc::now()`, `from = to - 24h`); `/api/bff/stations` + `/api/bff/stations/summary` paths, `BFF API` tag and validation unchanged
- [x] Frontend nginx log-level entrypoint renamed `20-log-level.sh` -> `19-log-level.sh` and pre-substitutes the template (fixes the e2e nginx startup failure)
- [x] Gates green: `make check`, `make test` (246), `make test-rest` (76), `make coverage` (overall 85.44%, core 96.93%), `make frontend-build`, `make test-e2e`

BFF endpoint separation + global summary + frontend polish (plans/25_bff_endpoint_separation_and_global_summary_plan.md)

- [x] Domain: `station_summary` refactored to a single `summarize(bounds: Option<GeoBounds>, from, to)` (optional map filtering, serving search + sidebar); `StationSummaryAggregate` removed
- [x] Domain: new decoupled `global_summary` module (`GlobalSummary` + `GlobalSummaryServicePort`) + application `GlobalSummaryService` (station/channel counts, last-24h total, most recent data-source-update timestamp)
- [x] BFF split into widget-named endpoints: `/api/bff/stations` (map markers, minimal), `/api/bff/stations/sidebar` (sidebar + visible/global counter), `/api/bff/stations/search` (all stations + action map), `/api/bff/global-summary` (header, outside `/stations/`)
- [x] Swagger/OpenAPI updated for the four BFF endpoints (paths, schemas, `BFF API` tag); obsolete `StationSummaryAggregateDto` / `StationSummaryListDto` removed
- [x] Frontend: map/sidebar/search/header each fetch their own endpoint; sidebar close button moved to the top of the sidebar, sidebar renamed "Visible counting stations", top-right counter shows visible/total, header shows the global summary + last update
- [x] Frontend: search-dialog clear button clears the filter instead of closing the dialog (separate Close button; Esc closes)
- [x] Removed the `[frontend] log_level` nginx feature completely (script, Dockerfile, nginx template, `config.toml(.example)`, compose mount)
- [x] Gates green: `make check`, `make test`, `make test-rest`, `make coverage`, `make frontend-build`, `make test-e2e`

Frontend component refactor + centered search bar (plans/27_frontend_component_refactor_and_centered_search_plan.md)

- [x] Split `frontend/src/App.tsx` into a feature-based structure under `frontend/src/features/` (header, map, sidebar, search, stations) with co-located components and data-fetching hooks
- [x] Add `frontend/src/lib/` (format.ts, geo.ts, leaflet.ts) holding the pure helpers and Leaflet bootstrap moved out of `App.tsx`
- [x] Turn `App.tsx` into a thin composition root that only owns bounds/mapRef/searchOpen/sidebarCollapsed, keyboard shortcuts, and focusStation
- [x] Center the header search trigger via a three-column grid in `frontend/src/index.css`
- [x] `make frontend-build` green (tsc + vite build); manual `make run` check of map/sidebar/search/find-on-map

Frontend shadcn/ui migration (plans/28_frontend_shadcn_ui_migration_plan.md)

- [x] Tailwind CSS v4 via `@tailwindcss/vite` (no tailwind.config.js); `@/*` path alias in `tsconfig.json` + `vite.config.ts`; `components.json`; `@types/node`
- [x] Rewrite `frontend/src/index.css` to the shadcn theme tokens (emerald primary on a slate neutral base, light default + `.dark` vars) with a base layer incl. the Leaflet preflight override
- [x] Add `src/lib/utils.ts` (`cn`) + shadcn components (`button`, `input`, `dialog`, `badge`, `scroll-area`, `separator`) + `lucide-react` icons
- [x] Convert App / TopBar / Sidebar / SearchDialog / StationListItem / MapView to shadcn components + Tailwind utilities; the hand-written global stylesheet is gone
- [x] Search dialog uses the Radix-based shadcn `Dialog` (z-index 2000, near-top position) preserving the overlay/sidebar/header stacking order
- [x] Sidebar list items: station name on top, description, and a readable stats row with emphasised numbers; Leaflet markers use a custom emerald (`#059669`) pin matching the header bar
- [x] `make frontend-build` green (tsc strict + vite build); `npm run dev` boots cleanly

Playwright end-to-end testing (plans/29_playwright_e2e_plan.md)

- [x] `@playwright/test` devDependency + `test:e2e` npm script in `frontend/package.json` (+ regenerated `package-lock.json`)
- [x] `frontend/playwright.config.ts` (Chromium, baseURL `FRONTEND_URL` default `http://localhost:8081`, list + html reporters, trace/screenshot/video on failure)
- [x] Map markers expose the station name via `alt`/`title` in `frontend/src/features/map/MapView.tsx` (test hook + a11y)
- [x] `frontend/e2e/map.spec.ts`: clicking a rendered map marker opens a popup with the same station name
- [x] `frontend/e2e/search.spec.ts`: search dialog filters "Bohlweg" and "Find on map" closes the dialog and opens its popup
- [x] `frontend/e2e/sidebar.spec.ts`: sidebar badge visible/total matches the rendered entries, marker count == sidebar item count, and zooming in shrinks the visible set
- [x] `scripts/e2e-playwright.sh`: boots the stack with a real Münster data source, waits for readiness + station import, runs Playwright, tears down + restores `config.toml`
- [x] Makefile `test-playwright` + `playwright-install` targets (`.PHONY` + help)
- [x] `.gitignore` Playwright artifacts (`frontend/test-results/`, `frontend/playwright-report/`, `frontend/blob-report/`)
- [x] Docs: `agents.md` (gate + Frontend e2e section + definition of done), `README.md` (Running tests), `ToDo.md`, `plans/29_playwright_e2e_plan.md` registered in `plans/README.md`
- [ ] Gates green: `make frontend-build`, `make check`, `make test-rest`, `make test-playwright`
