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

Quiet make output + local last-day summary (plan 30)

- [x] Quiet Make targets: `--quiet` on `cargo build` / `cargo test` / `cargo test-rest` / `cargo fmt`
- [x] Quiet `scripts/fmt-test.sh` (`--quiet` on fmt-check + clippy) and `scripts/coverage.sh` (`--quiet` on llvm-cov)
- [x] Redirect `docker compose up --build` to a temp log (tail only on failure) in `docker-compose-test.sh` / `e2e-playwright.sh`; silence `npm ci` / `playwright install`
- [x] Per-counting-station IANA `timezone` (domain VO + migration V11 + repository + Münster `parsing.rs` + import upsert)
- [x] `StationSummary.bikes_last_24h` -> `bikes_last_day`; `GlobalSummary.bikes_last_24h_total` -> `bikes_last_day_total` (domain + BFF DTO + frontend types)
- [x] DST-aware `previous_local_day` helper (chrono_tz) with unit tests (winter / spring-forward 23h / fall-back 25h)
- [x] `StationSummaryService` / `GlobalSummaryService` compute each station's previous local day in its own timezone
- [x] BFF handlers drop `last_24h_window()` and pass `now`; frontend text `bikes / 24 h` -> `bikes / last day`
- [x] Docs: `agents.md` (quiet convention), `README.md` (BFF field names + local-day semantics), `plans/30_..._plan.md` registered in `plans/README.md`
- [ ] Gates green: `make check`, `make test` / `make test-rest`, `make coverage`, `make test-playwright`

Counting-station overview page + provider images (plans/32_counting_station_overview_and_images_plan.md)

- [x] docker-compose: `minio` (minio/minio:latest, private `asset_network` with `internal: true`, no host port, `minio_data` volume, healthcheck) + one-shot `minio-init` bucket provisioning via the MinIO client `mc`; backend joins `default` + `asset_network` and depends on `minio` healthy + `minio-init` completed
- [x] Config: `[asset_storage]` (endpoint/access_key/secret_key/bucket/region) + `asset_cleanup_cron` (default `"0 0 4 * * *"`) + `asset_cleanup_max_lifetime_seconds` (required) + `AssetStorageConfiguration` value object + `Configuration` getters + TOML adapter parsing/tests + config.toml(.example) + scripts
- [x] Migration V12: `assets` table + `counting_stations.image_asset_id` FK (`ON DELETE SET NULL`) + `image_sha256` column
- [x] `CountingStation` gains `image_asset_id` + `image_sha256`; Postgres repo save/update/find + all in-memory mocks updated (REST `CountingStationDto` unchanged)
- [x] Assets domain: `Asset`/`AssetOrigin`/plain `BuiltinImage` + value objects, station-agnostic `AssetRepository` (incl. `all_object_keys`), `AssetStorage` (ensure/put/list/delete + async `get_stream`), `AssetServicePort`
- [x] Station-overview domain: `StationOverview`/`MetricWindow`/`MetricKey` + service port + DST-aware period helpers (`previous_local_days`, `previous_calendar_month`, `calendar_month_window`) with tests
- [x] Application services: `AssetService` (sync_builtin_images / default_asset / store_provider_image / find_by_id, sha2 hashing), `StationOverviewService` (day/7d/month windows + preceding periods + last_update), `AssetCleanupService` (asset_cleanup job type, orphan detection via list_object_keys - all_object_keys, job metadata)
- [x] Generic `ScheduledJobPort` driving port; `job_scheduler::run_scheduler(Arc<dyn ScheduledJobPort>, cron)` generalized and spawned twice in main.rs (data_source_update + asset_cleanup)
- [x] Built-in sample image `backend/assets/station-placeholder.jpg` (embedded via `include_bytes!`) + startup sync (ensure bucket + builtin images)
- [x] Driven adapters: `PostgresAssetRepository` + `MinioAssetStorage` (pinned `rust-s3 = "0.32"` with-tokio; blocking put/list/delete + streaming `get_stream` via ChunkWriter/mpsc)
- [x] Provider port: `image_sha256` on `CountingStationRecord` + default `get_station_image` returning `None`; hash-based image sync in `DataImportService.sync_counting_stations` (fetch only when hash changed, fallback to builtin default)
- [x] BFF `GET /api/bff/station-overview/{id}` (flat page payload — no HATEOAS `_links` / `data_source_id` / `CountingStationDto` reuse; trend up/down/flat + `delta_percent`) + `GET /api/bff/assets/{id}/content` (streaming with Content-Type/ETag/Content-Length/Cache-Control); AppState/router/OpenAPI wired
- [x] Frontend `features/stationOverview/` (types/api/useStationOverview/StationOverview/TrendIcon); `App` `selectedStationId` state (overview replaces sidebar, void-click closes); `MapView` marker-click select + popup detail link
- [x] Tests: core (period helpers, overview service, asset cleanup error paths, image sync), rest BFF (overview + asset content), Postgres asset repository, frontend e2e (popup link + overview open/void-click-close)
- [x] Docs: README (config + asset-storage subsection + docker-compose MinIO + BFF endpoints), ToDo, plans/32 (status + definition of done + implementation notes)
- [x] Docker build fixes found by the Playwright gate: `rust-s3` forces `openssl` via native-tls, and Alpine's `openssl-dev` has no static `.a`, so `backend/Cargo.toml` enables `openssl`'s `vendored` feature and the Dockerfile adds `perl make` + `COPY assets`; the map-void click moved into a `useMapEvents` child component (react-leaflet context)
- [x] Gates green: `make check`, `make test` (311), `make test-rest` (82), `make coverage` (overall 84.36%, core 95.20%), `make frontend-build`, `make test-playwright` (4 specs)

Overview detail-link polish + asset findings fixes (plan 33)

- [x] `AssetService::store_provider_image` rejects a provider hash that does not match the bytes (object key == persisted sha256) + unit test
- [x] `AssetObjectInfo` drops the unused `etag` (carries only `byte_size`); MinIO adapter no longer computes a duplicate hash; all mocks updated; port doc clarifies the BFF derives ETag from the DB sha256
- [x] `MinioAssetStorage::get_stream` uses a bounded `futures::channel::mpsc` channel; `ChunkWriter` applies Sink-based backpressure so a slow browser never buffers the whole object
- [x] `DataImportService::sync_counting_stations` resolves `default_asset()` once per import and passes it into `sync_station_image` (no per-station lookup)
- [x] Frontend: overview panel top bar is the clickable (non-link-styled) station name + `ExternalLink` icon button next to the "x" (in-body "Open detail page" text link removed); map popup uses the same name + icon affordance
- [x] Frontend: single `selectStation({ id, latitude, longitude })` entry point (map marker / sidebar / search "find on map" all fly + open the same overview panel); duplicate raw `L.popup` removed from `App.tsx`; `MapView.onSelectStation` receives the whole `StationMap`
- [x] e2e `map.spec.ts` updated to assert the icon link and the clickable heading (both `href` `/stations/{id}`)
- [x] Docs: `plans/33_..._plan.md` (status + definition of done) + registered in `plans/README.md`
- [x] Gates run: `make check`, `make test-rest` (82), `cargo test core::` (147), `make frontend-build`
- [ ] Docker gates pending: `make test`, `make coverage`, `make test-playwright`

Counting-station detail route + URL-encoded map/overview state (plan 34)

- [x] Add `react-router-dom` dependency; wrap the app in `BrowserRouter` in `main.tsx`
- [x] `App.tsx` becomes a `<Routes>` table: `/` → `MapPage` (extracted from the old composition), `/stations/:stationId` → blank `StationDetail`
- [x] New `features/map/MapPage.tsx` with the URL-state bridge: initial bbox + `station` read from the URL once, mirrored back with `setSearchParams(..., { replace: true })`
- [x] `lib/geo.ts` helpers: `parseBoundsQuery` (validate) + `serializeBounds` (round to 6 decimals)
- [x] `MapView` accepts `initialBounds` and fits it via `MapContainer` `bounds` (react-leaflet gives bounds priority when center/zoom are absent)
- [x] e2e `url.spec.ts`: bbox params in the URL, `station` appears/disappears with the overview, blank `/stations/:id` renders
- [x] e2e fixes surfaced by the gate: `search.spec.ts` now asserts the overview panel instead of the removed popup (plan 33 unified selection), and `sidebar.spec.ts` zooms until the visible set shrinks (a `moveend` re-render can drop a `+` keypress)
- [x] Gates green: `make frontend-build` (tsc + vite build), `make test-playwright` (6 specs), plus `make check` / `make test-rest` (backend untouched)

Counting-station detail page — layout + stats + graphs (plan 35)

- [x] Backend: calendar-year/week-start helpers (`calendar_year_window`, `previous_calendar_year`, `local_year_start`, `local_week_start`) in `counting_station.rs` (DST-aware)
- [x] Backend: `MetricKey::LastYear` (4th overview metric) + timezone-aware bucketed reads on `MeasurementRepository` (`sum_buckets`, `sum_buckets_by_channel`, `sum_weekdays`, `sum_by_channel` via PostgreSQL `date_bin`)
- [x] Backend: `station_detail` domain + `StationDetailService` (all windows, per-channel series incl. a timezone-aware per-channel weekday radar, channel pie, no zero-filling)
- [x] Backend: BFF `GET /api/bff/station-detail/{id}` + DTOs + OpenAPI; wiring in `main.rs` / `AppState` / `RestApiAdapter`
- [x] Frontend: shadcn `card`/`tooltip`/`chart` components + `recharts` / `@radix-ui/react-tooltip` deps
- [x] Frontend: shared `MetricCard` extracted from the overview panel + `last_year` label (overview + detail use the same component)
- [x] Frontend: `StationDetail` page — back-to-map link, half-page image + highlighted clickable map preview (opens the map view at the preview bounds via history push), name/description row, overview stat cards
- [x] Frontend: line charts via shadcn `chart` (last day 5 min, current + last week 15 min with the running week's empty tail, last 30 days 30 min + info note, current + last year 1 day, weekday radar) + Nerd-Stats per channel + channel pie
- [x] e2e `detail.spec.ts`: renders content, back-to-map → `/` with bbox, preview click → map view at preview bounds, browser back → `/stations/:id`; `url.spec.ts` updated (detail page is no longer blank)
- [x] Gates green: `make check`, `make test` (326), `make coverage` (overall 83.73%, core 95.34%), `make test-playwright` (9 specs)

Detail page fixes — header/search actions, resolutions, tooltip locale, legends (plan 36)

- [x] Backend: detail bucket resolutions corrected (week = 1 h, last 30 days = 1 day, `last_year` aligned to its own Jan 1) + unit test locking the widths
- [x] Backend: BFF search action map gains `open_detail` (always enabled) + `bff.rs` test
- [x] Frontend: central `LOCALE` in `lib/format.ts` + `formatFullDate` / `formatFullDateTime`; existing formatters use it
- [x] Frontend: `TimeSeriesLineChart` separates tooltip/axis formatters, drops empty series, renders the legend only with >1 non-empty series, empty-state message
- [x] Frontend: `StationDetail` full-date tooltips, corrected subtitles, empty-series filtering for the channel charts
- [x] Frontend: "Current + last week" / "Current + last year" (aggregate + per-channel) now overlap — buckets are re-anchored to the current week's Monday / the current year's Jan 1 (`alignSeries` + `weekdayAxis`)
- [x] Frontend: `ChannelPie` chart container given a real width (`w-full`) so the pie renders
- [x] Frontend: "Open detail" search action (StationListItem / SearchDialog / useStationSearch) alongside "Find on map"
- [x] Frontend: shared `SearchableHeader` used by map + detail page; header/search active on `/stations/:id`; find-on-map from the detail page flies via a station bbox (`stationBounds`)
- [x] e2e `detail.spec.ts`: header active, open-detail from search, share-by-channel pie renders
- [x] Gates green: `make check`, `make test` (327), `make coverage` (overall 83.74%, core 95.34%), `make test-playwright` (12 specs)

Detail page — shared timeframe selector, previous-period overlay, monthly bar chart (plan 37)

- [x] Backend: DST-aware `local_days_window(tz, now, n, offset_days)` helper + previous windows for the last day and the last 30 days; `MonthTotal` + `MeasurementRepository::sum_by_month` (Postgres GROUP BY year+month + testcontainers test)
- [x] Backend: `station_detail` restructured into per-timeframe `PeriodGraphs` (`day` / `week` / `last_30_days` / `year`, each with `current` / `previous` / `weekday_radar` / `channel_pie` / `per_channel`) + `monthly_totals`; `StationDetailService` + BFF DTOs updated
- [x] Frontend: shadcn `select` / `checkbox` / `label` components (`@radix-ui/react-select` / `-checkbox` / `-label`)
- [x] Frontend: shared timeframe dropdown + "Compare previous period" checkbox driving the full-width main line chart (previous period overlaid via a generalized `alignSeries`), the weekday radar, the channel pie and the nerd stats; the old per-window line-chart cards are dropped
- [x] Frontend: `MonthlyBarChart` (one bar per calendar month, grand total in the top-right) — standalone, not driven by the dropdown
- [x] e2e `detail.spec.ts`: timeframe dropdown swaps the main chart + monthly bar chart renders; compare-previous checkbox overlays the previous period
- [x] Gates green: `make check`, `make test` (332), `make test-rest` (82), `make coverage` (overall 83.81%, core 95.36%), `make test-playwright` (14 tests)

Bike icon branding + builtin asset folder sync (plan 38)

- [x] Both bike SVGs (`bike-icon-white-circle.svg`, `bike-icon-black-transparent.svg`) recolored from black to the header emerald `#059669`
- [x] `builtin_images()` registers both SVGs (`image/svg+xml`); `DEFAULT_IMAGE_OBJECT_KEY` repointed to the plain bike icon (`builtin/bike-icon-black-transparent.svg`)
- [x] `backend/assets/station-placeholder.jpg` removed from the repo
- [x] `AssetRepository::delete(object_key)` added to the port + `PostgresAssetRepository` (Postgres test included)
- [x] `AssetService::sync_builtin_images` now reconciles both directions: uploads/updates bundled images **and removes** builtin assets no longer in the folder (DB row first, then object; `ON DELETE SET NULL` unlinks stations, BFF/import fall back to the new default); unit test for stale-builtin removal + provider assets kept
- [x] Frontend: `public/bike-icon.svg` + favicon link in `index.html`; header brand icon (was the 🚴 emoji) in `TopBar`; Leaflet zoom control moved to the right in `MapView`
- [x] Frontend (found by the e2e gate): `WeekdayRadar` empty-state guard — recharts `RadarChart` crashed (`Cannot read properties of null (reading 'map')`) on an empty/all-zero window, now shows "No traffic for this period."
- [x] e2e: `detail.spec.ts` compare-previous assertion made robust — accepts either the "Day before" legend entry (current day has data) or the single previous-period series (empty current day)
- [x] Docs: README (built-in asset description), ToDo, `plans/38_..._plan.md` registered in `plans/README.md`
- [x] Gates green: `make check`, `make test` (334), `make test-rest` (82), `make coverage` (overall 83.78%, core 95.38%), `make frontend-build`, `make test-playwright` (14)

Frontend resilience + builtin asset folder sync (plan 39)

- [x] Frontend: `ErrorBoundary` class component (`getDerivedStateFromError` + `componentDidCatch`, console-logged) wrapping `<DetailContent>` in `StationDetail`, so a chart/render crash degrades to an inline error while the header/search keep working (no more full-page blank)
- [x] Frontend: shared `ChartEmptyState` component; `TimeSeriesLineChart`, `ChannelPie` and `WeekdayRadar` all use it and guard empty/all-zero data (consistent wording + aspect ratio)
- [x] Backend: `include_dir` embeds `backend/assets/` at compile time; `builtin_images()` now scans the folder (object_key `builtin/{path}`, content type derived from the extension via a new forward helper) — adding/removing a file in the folder is the only step needed; `map-flag-counting-station.svg` now gets synced too (matches "sync the folder")
- [x] Backend: unit tests for the extension→content-type mapping (incl. case-insensitivity + unknown) and the folder-scan derivation (default icon present, white-circle gone, deterministic sort)
- [x] Backend: `backend/assets/bike-icon-white-circle.svg` removed — the brand white-circle icon now lives only in `frontend/public/bike-icon.svg` (favicon + header)
- [x] e2e volumes stay persistent (no `docker compose down -v`) — the real Münster import is not re-run per gate; data-dependent assertions from plan 38 remain robust
- [x] Docs: README (asset folder-scan + single brand icon), ToDo, `plans/39_..._plan.md` marked implemented in `plans/README.md`
- [x] Gates green: `make check`, `make test`, `make test-rest` (82), `make coverage`, `make frontend-build`, `make test-playwright`

Map-marker flag relocation + detail-page default week (plan 40)

- [x] `map-flag-counting-station.svg` recolored from black to the emerald `#059669` and relocated from `backend/assets/` to `frontend/src/features/map/` (Vite-imported in `frontend/src/lib/leaflet.ts`)
- [x] Both `stationIcon` (map) and `detailStationIcon` (detail preview) now use the flag; editing the SVG file updates the markers without a code change
- [x] `backend/assets/map-flag-counting-station.svg` deleted — the compile-time folder scan + add/remove sync stops syncing it to object storage
- [x] Detail page default timeframe changed from 24 hours to "Current + last week" (the 24-hour window is not always populated)
- [x] e2e `detail.spec.ts` default-bucket + compare-previous assertions updated for the week default
- [x] e2e `sidebar.spec.ts` focus-click moved to map void + overview-close guard (the wider 32px marker hit area covered the old click point and opened the overview)
- [x] Docs: README (marker asset convention), ToDo, `plans/40_..._plan.md` registered in `plans/README.md`
- [x] Gates green: `make check`, `make test` (337), `make test-rest` (82), `make coverage` (overall 84.08%, core 95.38%), `make frontend-build`, `make test-playwright` (14)

Station summary view — aggregate visible stations (plan 41)

- [x] Backend: `stations_summary` domain module (new DO `StationsSummary` + per-station graph types) + `StationsSummaryServicePort`
- [x] Backend: `StationsSummaryService` (bounds/exclude filtering, per-station timezone-aware overview metrics, aggregated bucketed graphs + per-station series, derived from one `sum_buckets_by_channel` scan per period per timeframe) + core unit tests
- [x] BFF `GET /api/bff/stations/summary` (required bounds + optional `exclude`, fallback image, page-shaped DTOs) + OpenAPI path/schemas + AppState/router/main wiring
- [x] BFF endpoint tests (`rest/tests/bff.rs`: page shape, inverted bounds 400, exclude parsing, metric aggregation)
- [x] Frontend refactor for reuse: shared `stationDetail/timeframes.ts` extracted from `StationDetail`; `ChannelPie` generalized into `SharePie` (no behaviour change)
- [x] Frontend: `features/stationsSummary` (types/api/`useStationsSummary` with loading state/`StationsSummary` page/`SummaryMap` with click-to-disable + grayed `disabledStationIcon`); `/summary` route; `disabled` URL param; sidebar pinned "Summarize visible stations" footer (list stays scrollable)
- [x] e2e `summary.spec.ts` (navigation from sidebar, render, disable toggle + URL param, shared URL restore, back-to-map) — scoped to one station so the browser stays fast
- [x] Gates green: `make check`, `make test` (351), `make test-rest` (87), `make coverage` (overall 84.98%, core 95.78%), `make frontend-build`
- [x] Gates green: `make test-playwright` (19 specs pass in 20.1s, incl. the 5 new summary specs)

Known limitations / tech debt recorded once for this plan (genuine findings, not speculative):

- The full-view summary aggregates all visible stations (~23 × 70 channels) on the fly; the payload (~2 MB) and client rendering are heavy, so the page shows a loading state. The planned optimisation is a cache (e.g. Redis) behind the BFF — no new data fields were added so the shapes stay cacheable.
- The bucketed charts for a group of stations run in the first included station's timezone; the overview metrics remain per-station timezone-correct. A mixed-timezone group would only shift the chart buckets.
- During implementation an infinite reload loop was found and fixed: `parseBoundsQuery` built a fresh object each render, so the summary data hook re-ran (and reset the loading state) on every render — fixed by memoizing `bounds` on the search params.

All-time bike counter + latest-year trend removal (plan 45)

- [x] Backend: `total_bikes` (all-time) added to the `StationOverview` and `StationsSummary` domain structs
- [x] Backend: `StationOverviewService` computes it via `sum_by_month`; `StationsSummaryService` derives it from `graphs.monthly_totals`
- [x] BFF: `total_bikes` exposed on the `station-overview`, `station-detail` and `stations/summary` payloads + endpoint tests
- [x] Frontend: shared `TotalBikesCard` (`Total bikes (all time)`) rendered on the overview panel, detail page and summary page
- [x] Frontend: `MonthlyBarChart` no longer shows a trend on the latest (always-incomplete) year button
- [x] e2e: counter presence asserted in `map.spec.ts` / `detail.spec.ts` / `summary.spec.ts`; latest-year button shows no p-%
- [x] Gates green: `make check`, `make test` (351), `make test-rest` (87), `make coverage` (overall 85.06%, core 95.80%), `make frontend-build`, `make test-playwright` (20)

Hour-of-day radar + weekday axis label fix + radar compare-previous (plan 48)

- [x] Backend: `HourTotal` / `ChannelHourTotal` + `sum_hours` / `sum_hours_by_channel` on the `MeasurementRepository` port; Postgres SQL via `EXTRACT(HOUR …)` with tests; stubs in all 10 test repository mocks
- [x] Backend: `weekday_radar_previous` / `hourly` / `hourly_previous` on the detail (`PeriodGraphs`, `PerChannelSeries`) and summary (`SummaryPeriodGraphs`, `PerStationSeries`) domain models, computed in both services (aggregate + per channel/station) with unit tests
- [x] BFF: `HourTotalDto` + the new fields on the detail/summary graph DTOs + OpenAPI schemas + `bff.rs` payload assertions
- [x] Frontend: `HourTotal` + new fields in the detail/summary types; new `HourRadar` component (fixed 24-hour radar, multi-series, empty state)
- [x] Frontend: Weekdays + Hours radars split half/half on the detail/summary "Detailed statistics" and nerd-stats sections; both radars honor the compare checkbox (aggregate + per channel/station)
- [x] Frontend: week line-chart X-axis fixed — `weekdayAxis` now appends the local time so hour-level ticks are unique (was `Mo Mo Mo Mo Di …`)
- [x] e2e: `detail.spec.ts` hour-radar card test + `summary.spec.ts` Hours-card assertion
- [x] Gates green: `make check`, `make test` (363), `make test-rest` (87), `make coverage` (overall 85.44%, core 96.10%), `make frontend-build`, `make test-playwright` (21)

Station analytics consolidation + measurement sum N+1 fix (plan 49)

- [x] Backend: `MeasurementRepository::sum` now takes `&[ChannelId]` — one `ANY($1)` query replaces the per-channel N+1 loops; Postgres impl + all in-memory mocks updated
- [x] Backend: the five station modules (`station_summary`, `stations_summary`, `station_overview`, `station_detail`, `global_summary`) consolidated into one `station_analytics` domain module with a single `StationAnalyticsServicePort`
- [x] Backend: one `StationAnalyticsService` (summaries / global_summary / overview / detail / stations_summary) sharing `sum_window`, `weekday_totals`, `metric_windows`, `period_data`, `graph_windows` and `last_update`
- [x] Backend: the five per-service test modules merged into one shared in-memory test double; all unit tests ported
- [x] Backend: BFF handlers use the single service with a shared `station_image_url` helper; `bff/dto.rs` graph conversions deduplicated (`buckets`/`weekdays`/`hours`/`months`)
- [x] Wiring: `main.rs` / `AppState` / `RestApiAdapter` / `tests/mocks.rs` use `StationAnalyticsService`
- [x] Gates green: `make check`, `make test` (363), `make test-rest` (87), `make coverage` (overall 86.03%, core 95.85%)

Sidebar shell + stats sub-resource + Skeleton loading states (plan 52)

- [x] Backend: `sidebar_shell` / `sidebar_stats` on `StationAnalyticsServicePort` + `StationAnalyticsService` (extracted `stations_for_bounds` / `channel_maps` / `bikes_by_station` helpers) + `SidebarStationStats` domain model
- [x] Backend: `GET /api/bff/stations/sidebar` now returns the shell (identity + `image_url` + counters + `_links.stats`); new `GET /api/bff/stations/sidebar/stats` returns `channel_count` + `bikes_last_day` per station; `SidebarShellDto` / `SidebarStationDto` / `SidebarStatsDto` / `SidebarStationStatsDto` replace `StationSummarySidebarDto`; `station_image_urls` batch helper; router + OpenAPI updated
- [x] Backend: bff.rs + core `station_analytics` tests updated/added; search endpoint + `StationListItem` unchanged; measurement domain/repositories untouched
- [x] Frontend: shadcn `Skeleton` primitive + shared `stationDetail/Skeletons.tsx` (`PageShellSkeleton` / `OverviewSkeleton` / `ChartsSkeleton` / `MonthlyBarSkeleton`)
- [x] Frontend: sidebar shell/stats split in `stations/types.ts` + `api.ts` + `useVisibleStations.ts`; new `SidebarListItem` (image + name render directly, skeleton stats line); `Sidebar` shows a skeleton list while the shell loads
- [x] Frontend: detail/summary pages (and the overview panel) render skeleton cards while loading and fill on arrival, with per-card error states
- [x] e2e `sidebar.spec.ts` asserts the image thumbnail + stats line; `summary.spec.ts` comment updated
- [x] Gates green: `make check`, `make test` (373), `make test-rest` (93), `make coverage` (overall 87.53%, core 95.98%), `make frontend-build`, `make test-playwright` (21)

Station-overview shell + stats sub-resource (plan 53)

- [x] Backend: `overview()` / `StationOverview` replaced by `overview_shell()` / `StationOverviewShell` (station + channel count + last update, no aggregation) on the port + service; `detail_overview_stats` now computes `metric_windows` + `sum_by_month` itself (was delegating to `overview`)
- [x] Backend: `GET /api/bff/station-overview/{id}` now returns the shell (identity + `image_url` + `_links.stats`); new `GET /api/bff/station-overview/{id}/stats` returns `total_bikes` + `metrics` via `StationOverviewStatsDto`; router + OpenAPI updated
- [x] Backend: bff.rs + core `station_analytics` tests updated (shell shape, stats endpoint, 404s, timezone metrics via `detail_overview_stats`)
- [x] Frontend: `stationOverview` types/api/hook split into `StationOverviewPage` (shell) + `StationOverviewStats`; the panel renders the name/image/description immediately and shows a stats `Skeleton` until the parallel stats sub-resource arrives
- [x] Gates green: `make check`, `make test` (375), `make test-rest` (95), `make coverage` (overall 87.57%, core 95.98%), `make frontend-build`, `make test-playwright` (21)

Align overview loading skeletons with the rendered cards (plan 54)

- [x] Frontend: `stationDetail/Skeletons.tsx` exports `MetricBoxSkeleton` (right column gap tightened to `gap-0.5`) and adds `TotalBikesSkeleton` mirroring `TotalBikesCard`'s bordered `bg-muted/40 p-4` box
- [x] Frontend: new `stationOverview/Skeletons.tsx` `OverviewPanelSkeleton` (total-card skeleton + four metric-box skeletons in the same `gap-2` column as the rendered cards); the overview panel uses it for both the shell and stats loading states, and the shell ghost now includes the badge/updated row
- [x] Gates green: `npm run build` (tsc + vite), `make test-playwright` (21)

Map control scrollbar flicker (plan 62)

- [x] Frontend: contain full-screen map-page overflow so the sidebar's temporary loading-state overflow cannot create a document scrollbar and shift the top-right MapLibre navigation control.

Small UI + script fixes (plan 55)

- [x] Makefile: replaced `cd <dir> && <tool>` with `--manifest-path` / `--prefix` for build, fmt, test, test-rest, clean, playwright-install and frontend-build
- [x] scripts/e2e-playwright.sh: removed `cd` (`readlink -f` for SCRIPT_DIR/PROJECT_ROOT, `npm ci --prefix`, `npm exec --prefix -- playwright …` with `--config`); the temporary-file `rm` calls are grouped in `cleanup()`; three milestones print ("Stack built." / "Stack started (app ready)." / "Tests finished.")
- [x] Frontend: `TopBar` stays on a single line — brand logo/text links to `/`, the search trigger is widened to `w-[32rem]` (magnifier only, no bike icon), and the global summary truncates with an ellipsis instead of wrapping
- [x] Backend + frontend: each search result row shows the station image (bike-icon fallback) via `StationSummaryDto.image_url` + `StationSummary.image_url` + a `StationListItem` thumbnail
- [x] Frontend: the header summary keeps only the "updated …" timestamp when space is tight (stats truncate, timestamp is `shrink-0`), and search-result rows highlight across the full row on hover (`li` hover instead of the button)
- [x] Gates green: `make check`, `make test-rest` (95), `npm run build` (tsc + vite)

Bundle the tiles init into the backend startup (plan 65)

- [x] Backend: `MapsConfiguration` + `[maps]` TOML section (`update_cron` default every two months, `update_max_lifetime_seconds` default 2 hours, pinned `protomaps_build_url` + `go_pmtiles_version`) added to `Configuration`, the TOML adapter, `config.toml.example` and the test-script configs
- [x] Backend: `TilesInit` driven adapter (`backend/src/adapter/driven/tiles_init/`) reuses the official `go-pmtiles` CLI (downloaded at runtime, cached) to extract world z0-5 + Germany bbox (hard-coded) and merge into `map.pmtiles`; `ensure_available()` at startup (mandatory, no skip) + atomic `update()` (build to temp, then rename); subprocess output streamed to stdout
- [x] Backend: `TilesProvisioningPort` (domain) + `TilesUpdateService` (`tiles_update` ShedLock-style job on the `[maps]` cron)
- [x] Backend: `main.rs` ensures tiles in the init phase before the server binds; `bike_counter tiles` subcommand + `entrypoint.sh` arg forwarding for `make tiles` / `make tiles-update`
- [x] Infra: `docker-compose.yml` removes the `tiles` service, mounts `./tiles:/data` into the backend, and the frontend waits on `backend: service_healthy`; Makefile `tiles`/`tiles-update` use `docker compose run --rm --no-deps backend tiles`; `run` drops the `tiles` prerequisite; smoke-test readiness wait raised for the first-run basemap build
- [x] Docs: `tiles/README.md`, `README.md`, `ToDo.md`, `plans/65_..._plan.md` + `plans/README.md`
- [ ] Gates green (pending local run): `make check`, `make test`, `make test-rest`, `make coverage`, `make test-e2e`, `docker compose config`

Back to map preserves the previous view (plan 67)

- [x] Summary page (`StationsSummary.tsx`): the "Back to map" link rebuilds `/?<bounds>` from the bounds already parsed out of the `/summary` URL (`to={bounds ? \`/?${serializeBounds(bounds)}\` : '/'}`) instead of a plain `/`, so the map re-opens at the previously visible area
- [x] Detail page (`StationDetail.tsx`): the "Back to map" link history-backs when the page was reached via in-app navigation (`location.key !== 'default'` → `preventDefault()` + `navigate(-1)`), keeping `href="/"` + a plain navigate for shared/deep links (key `'default'`); documented edge case `map -> summary -> detail -> back` returns to `/summary`
- [x] e2e: `summary.spec.ts` back-to-map assertion compares the restored bbox against the `/summary` URL bbox (`toBeCloseTo`); `detail.spec.ts` adds an in-app back-to-map test that returns to the previous map view
- [x] Gates green: `npm run build` (tsc + vite), `make test-playwright` (23)

Dependency, base-image upgrade + cargo audit (plan 68)

- [x] Frontend: all `dependencies`/`devDependencies`/`engines` + `.nvmrc` bumped to latest stable (React 19.2, Vite 8, TypeScript 7, recharts 3.10, pmtiles 4.5, …); `package-lock.json` regenerated; `npm run build` green after recharts 3 refactors (`chart.tsx` TooltipContentProps/DefaultLegendContentProps, `String(item.dataKey)`, `import.meta.dirname` in vite.config)
- [x] Backend: `Cargo.toml` bumped to latest stable majors (axum 0.8, ureq 3, utoipa 5, rust-s3 0.37, refinery 0.9, toml 1.1, cron 0.17, sha2 0.11, zip 8.6, tower 0.5, testcontainers 0.27); breaking-API refactors (axum `{id}` route paths, ureq 3 `into_body()`, rust-s3 `Box<Bucket>` + `ResponseDataStream`, sha2 hex via iter, OpenAPI 3.1.0)
- [x] Docker: backend `alpine` latest, frontend `node` LTS + `nginx` latest, compose `postgres:18-alpine` (data volume now `/var/lib/postgresql` for PG 18; documented major-upgrade volume caveat); pinned `go_pmtiles_version`/`protomaps_build_url` bumped across configs + backend defaults/tests
- [x] cargo audit: `scripts/audit.sh` + `make audit` wired into `make check`; `backend/.cargo/audit.toml` ignores the two unfixable `quick-xml` advisories via rust-s3/aws-creds with justification; documented in `agents.md`
- [x] Gates green: `make check` (incl. audit), `make test` (446), `make test-rest` (95), `make coverage` (overall 87.11%, core 95.04%), `npm run build`, `make test-e2e`, `make test-playwright` (23)

Isolate PostgreSQL on an internal Docker network (plan 69)

- [x] `docker-compose.yml`: new internal `db_network` (`internal: true`), Postgres moved onto it, the `5432:5432` host port removed, and the backend attached to `default` + `asset_network` + `db_network`; db healthcheck kept
- [x] Postgres is now reachable only from inside Docker (backend + `docker compose exec db psql …`); `make test-e2e` (compose smoke test) and `make test-playwright` (23) green with the isolated db
- [x] Docs: README (Postgres isolation + local `psql` via `docker compose exec`), ToDo, `plans/69_..._plan.md` + `plans/README.md`
- [x] **Data-recovery note**: re-creating the volume for the PG18 bump wiped all imported data (stations/measurements). Restored the full three-source `config.toml` (Münster + Bonn + Hamburg from `config.toml.example`) and re-imported from the public APIs; the Hamburg backfill is large and intermittently throttled upstream, so the hourly job keeps retrying (see plan 68 retry/URL fixes). Documented in `plans/69_..._plan.md`.
