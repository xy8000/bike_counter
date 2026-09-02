# 88 — Data-sources UI

## Goal

Add a data-sources section to the Bike Counter frontend, reachable from the top bar
(desktop + mobile). It shows one overview section per provider (last successful
import, station count, channel count) and, on click, a detail page (large image,
map of the provider's stations, a Data-Overview with import facts + feature
badges). Each data source has a replaceable image served by its adapter, falling
back to a recolored data-source SVG.

## Backend

### Data model (migrations)

- **V18**: extend `data_sources` with `logo_asset_id UUID NULL REFERENCES assets(id)
  ON DELETE SET NULL` and `logo_sha256 TEXT NULL` (mirrors
  `counting_stations.image_asset_id` / `image_sha256`).
- **V19**: new `data_source_imports` table — one row per data source per update run:
  - `id UUID PK`
  - `data_source_id UUID NOT NULL REFERENCES data_sources(id) ON DELETE CASCADE`
  - `job_id UUID NULL REFERENCES jobs(id)`
  - `started_at TIMESTAMPTZ NOT NULL`, `finished_at TIMESTAMPTZ NULL`
  - `status TEXT NOT NULL` (`RUNNING` | `FINISHED` | `FAILED`)
  - `failure_message TEXT NULL`
  - `warning_count INT NOT NULL DEFAULT 0`, `error_count INT NOT NULL DEFAULT 0`
  - `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

### Domain

- Extend [`DataSource`](../backend/src/core/domain/data_source/data_source.rs) with
  `logo_asset_id: Option<AssetId>` and `logo_sha256: Option<String>`.
- New `DataImportRun` aggregate + `DataImportRunRepository` port + a
  `DataImportRunServicePort` (start / finish / fail / counts), plus
  [`service_port`](../backend/src/core/domain/data_source/service_port.rs) entries.
- Add `DataProvider::get_data_source_image()` returning
  `Option<DataSourceImage>` (`sha256`, `content_type`, `bytes`), default `None`,
  in [`provider_port.rs`](../backend/src/core/domain/data_source/provider_port.rs).

### Repositories (ports + Postgres)

- [`CountingStationRepository`](../backend/src/core/domain/counting_stations/repository_port.rs):
  `find_by_data_source_id` + `count_by_data_source_id`.
- [`ChannelRepository`](../backend/src/core/domain/channels/repository_port.rs):
  `find_by_data_source_id` (join `counting_stations` on `data_source_id`) + `count_by_data_source_id`.
- [`ProviderMessageStore`](../backend/src/core/domain/data_source/provider_message_port.rs):
  `count_by_severity_since(data_source_id, since)` → warning/error counts for the
  last import.
- New `DataImportRunRepository` Postgres implementation.

### Application services

- New `DataSourceAnalyticsService` that, for a data source, computes:
  - `station_count`, `channel_count`
  - `last_updated_at` (last successful import, already persisted)
  - `first_data_at` / `last_data_at` via `earliest_by_channel` / `latest_by_channel`
  - feature badges (data-derived, per user decision):
    - **historical data**: `first_data_at` older than 365 days before now
    - **real-time data**: `last_data_at` within 24 hours of now
    - **full current year coverage**: every calendar month of the current year
      (Jan..current month) present in `sum_by_month`
  - last import run facts (status / duration / warning_count / error_count).
- [`DataSourceUpdateService`](../backend/src/core/application/data_source_update_service.rs)
  records one `DataImportRun` per source (start before, finish/fail after
  `update_data_source`), counts provider WARNING/ERROR messages since the run
  started, and stores the provider logo through
  [`AssetService`](../backend/src/core/application/asset_service.rs)
  (`store_provider_image`) when `get_data_source_image()` returns a new hash.

### BFF

- DTOs: `DataSourceListItemDto` (id, name, provider_type, last_updated_at,
  station_count, channel_count, image_url) and `DataSourceDetailDto` (id, name,
  provider_type, image_url, stations for the map, stats, badges, last-import
  status/duration/warning_count/error_count/failure_message).
- Handlers + routes:
  - `GET /api/bff/data-sources`
  - `GET /api/bff/data-sources/{id}`
- Image streaming reuses [`get_bff_asset_content`](../backend/src/adapter/driving/bff/handlers.rs:936);
  `image_url` is empty when no logo so the frontend renders the SVG fallback.

## Frontend

- Recolor [`example/connected-nodes-to-the-cloud-svgrepo-com.svg`](../example/connected-nodes-to-the-cloud-svgrepo-com.svg)
  from `#000000` to the brand emerald `#059669` (the bike color in
  [`bike-icon.svg`](../frontend/public/bike-icon.svg:12)) and add it under
  `frontend/src/features/dataSources/data-source.svg` (Vite asset).
- New `features/dataSources`: `types.ts`, `api.ts`, hooks, `DataSourcesList.tsx`,
  `DataSourceDetail.tsx`, `DataSourceMap.tsx` (reuses
  [`BaseMap`](../frontend/src/features/map/BaseMap.tsx) +
  [`stationMarkerImage`](../frontend/src/lib/map.tsx:46)), skeletons.
- Detail page layout mirrors [`StationDetail`](../frontend/src/features/stationDetail/StationDetail.tsx:168):
  back link, large image (SVG fallback), map of the provider's stations, then the
  Data-Overview (stats + import duration + first data from + warning/error
  counters + a failed-import warning + feature badges).
- Top bar: add a "Data sources" entry (icon on mobile, labelled link on `sm+`) to
  [`TopBar`](../frontend/src/features/header/TopBar.tsx:11), adjusting the grid.
- Routes: `/data-sources` and `/data-sources/:dataSourceId` in
  [`App.tsx`](../frontend/src/App.tsx:11).

## Tests & gates

- Core unit tests for `DataSourceAnalyticsService`, `DataImportRun` lifecycle and
  the new repository ports (in-memory doubles); Postgres repository tests for the
  new queries; BFF/REST tests (including OpenAPI paths) in
  [`bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:1).
- Playwright e2e specs for the list + detail pages; extend the seeded fixture if
  needed.
- Gates: `make check`, `make test`, `make test-rest`, `make coverage`,
  `make test-playwright`.
- Docs: update [`README.md`](../README.md), [`ToDo.md`](../ToDo.md) and register
  this plan in [`plans/README.md`](../plans/README.md).

## Flow

```mermaid
flowDiagram TD
  A[Update job] --> B[Per-source DataImportRun]
  B --> C[Provider logo via AssetService]
  B --> D[Provider messages counted per severity]
  E[DataSourceAnalyticsService] --> F[overview + detail + badges]
  F --> G[BFF data-sources endpoints]
  G --> H[Frontend list and detail pages]
```

## Implementation status

Implemented and locally validated:

- **Backend**: migrations V18 (data-source logo) + V19 (`data_source_imports`),
  `DataSource.logo_asset_id`/`logo_sha256`, `DataImportRun` domain + Postgres
  repository, `DataProvider::get_data_source_image` hook, per-data-source
  counting-station/channel/provider-message/measurement queries, the
  `DataSourceAnalyticsService` (overview/detail + badges + last-import facts),
  per-source run recording in `DataSourceUpdateService`, BFF DTOs/handlers/routes
  `GET /api/bff/data-sources` (+`/{id}`), OpenAPI registration and tests.
  Validated with `cargo check`, `cargo fmt`, `cargo clippy --all-targets -- -D
  warnings`, the in-memory REST suite (109 tests) and the full Postgres suite (47
  tests, Docker testcontainers) which also proves the V18/V19 migrations apply.
- **Frontend**: recolored `data-source.svg` (#059669), the `features/dataSources`
  pages (list + detail + map), top-bar navigation and the two routes. Validated
  with `tsc --noEmit`, `vite build` and prettier.

### Rapid-feedback round (list + detail polish, Hamburg latency)

- **Data-source overview is now an aligned table-like grid** (shared column
  template between the header and every row so nothing overflows) with compact
  paddings, plus a small info box explaining the rows/status.
- **Data-source detail** uses compact stat boxes (no large card paddings /
  free-space), an `Import duration` card that shows *Unknown* while an import is
  still running, and aligned loading ghosts that mirror the real layout (image +
  map row, badges and the 2/4 stat-card grid).
- **Hamburg latency**: the detail endpoint no longer aggregates the source's
  data. First/last data come from the per-channel index seeks
  (`earliest_by_channel` / `latest_by_channel` — the former switched from a
  full-scan `GROUP BY MIN` to `DISTINCT ON … ORDER BY timestamp ASC`), and the
  "full current year coverage" badge probes one cheap index seek per elapsed
  calendar month (`has_measurements_in_windows`) instead of summing the whole
  current year. This replaced the earlier
  `measurement_bounds_by_data_source`/`sum_by_month_range` pair (both scanned
  large swathes of history). The committed seed fixture still needs regenerating
  against V18/V19 (see follow-ups).

## Follow-ups (not run in this environment)

- **Regenerate the Playwright seed fixture** against the new schema
  (`scripts/dump-e2e-fixture.sh` after booting the stack) — the committed
  [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql) is a full pg_dump
  (schema + refinery history up to V17) and must include V18/V19 before
  `make test-playwright` passes. The new
  [`frontend/e2e/data-sources.spec.ts`](../frontend/e2e/data-sources.spec.ts)
  depends on it.
- Run the remaining gates: `make test`, `make coverage`, `make test-playwright`.
- Update [`README.md`](../README.md) / [`ToDo.md`](../ToDo.md) for the new
  data-sources pages.
