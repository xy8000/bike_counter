# 87 - BFF API audit fixes

Status: implemented (backend, with documented deviations — see below)

## Problem

An audit of the BFF API (everything under `/api/bff`, consumed only by the React
frontend) found a set of scalability, consistency, HTTP-caching and error-handling
flaws. The endpoints are functionally correct, but several of them load the full
station/channel tables and issue one measurement query per station, a few
sub-resources ignore the `as_of`/`exclude_new_stations` contract, JSON responses
advertised as cacheable carry no caching headers, and 500 responses leak internal
database/provider details.

## Flaw inventory

| # | Severity | Area | Finding | Evidence |
|---|---|---|---|---|
| F1 | high | performance | Bounds filtering is done in Rust over the full station table instead of being pushed down to Postgres. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:234), [`station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:248), [`station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:89), [`station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:109) |
| F2 | medium | performance | The sidebar `total_count` loads every station just to call `.len()`. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:287) |
| F3 | high | performance | `bikes_last_day` issues one `sum_window` query per station (N+1); the search endpoint does this for every station in the system. | [`station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:287), [`station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:379) |
| F4 | medium | performance/design | `GET /api/bff/stations/search` returns every station unbounded and computes per-station `bikes_last_day` for all of them. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:371) |
| F5 | medium | consistency | The monthly sub-resources do not consistently honor `as_of`: the detail monthly handler ignores it (uses `Utc::now()`) and its HATEOAS link omits it; the summary monthly link omits `as_of` even though the handler accepts it. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:584), [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:433), [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:732) |
| F6 | medium | consistency | `GET /api/bff/station-overview/{id}/stats` hardcodes `exclude_new_stations=false`, so the map-popup overview can disagree with the detail overview card under the Bike-Trends setting; it also ignores `as_of`. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:698) |
| F7 | low | semantics | A custom `from`/`to` range silently overrides the `{timeframe}` path segment, making `/graphs/day?from=..&to=..` misleading. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:547), [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:854) |
| F8 | medium | HTTP | Windowed JSON responses are documented as cacheable but carry no `Cache-Control`/`Vary`/`ETag`; the asset endpoint sets an `ETag` but never answers `If-None-Match` with `304`. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:938) |
| F9 | high | security | 500 responses embed raw database and provider error text, leaking internals. | [`rest/handlers/mod.rs`](../backend/src/adapter/driving/rest/handlers/mod.rs:65) |
| F10 | low | hygiene | `StationOverviewDto.detail_url` hardcodes the frontend route; `StationSearchDto.actions` is always both-enabled dead weight; `StationSummaryDto` flattens `data_source_id`/`_links` the search dialog never uses. | [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:669), [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:393), [`bff/dto.rs`](../backend/src/adapter/driving/bff/dto.rs:36) |

## Goal

Fix the BFF API so that (a) viewport-scoped reads are bounded by the database, (b)
per-station aggregations are batched into single queries, (c) every windowed
sub-resource honors the same `as_of`/`exclude_new_stations` contract, (d) cacheable
responses actually advertise cacheability and honor conditional requests, and (e)
errors do not leak internals. No frontend-visible response-shape changes.

## Implementation notes

Implemented in this pass (backend, fully backward compatible — no frontend
changes required):

- **F1** — bounds filtering pushed into Postgres via
  `CountingStationRepository::find_in_bounds` (SQL `BETWEEN`) for the map
  ([`list_bff_stations`](../backend/src/adapter/driving/bff/handlers.rs:228)),
  the sidebar/summary shells and the analytics helpers
  ([`stations_for_bounds`](../backend/src/core/application/station_analytics/service.rs:248),
  [`included_stations`](../backend/src/core/application/station_analytics/service.rs:89),
  [`summary_stations_in_bounds`](../backend/src/core/application/station_analytics/service.rs:109)).
- **F2** — the sidebar `total_count` uses the new `count_all()` SQL count
  ([`get_bff_stations_sidebar`](../backend/src/adapter/driving/bff/handlers.rs:287)).
- **F3** — `bikes_last_day` (and the global header sum) now groups stations by
  timezone and runs one `sum_by_channel` query per timezone instead of one query
  per station ([`bikes_by_station`](../backend/src/core/application/station_analytics/service.rs:307),
  [`global_summary`](../backend/src/core/application/station_analytics/service.rs:391));
  the Bike-Trends `earliest_by_channel` lookup in the header is one query across
  every channel.
- **F5** — the detail-monthly handler and the overview-stats sub-resource accept
  `as_of`/`exclude_new_stations`, and both monthly HATEOAS links now pin `as_of`,
  so every windowed card is a pure function of its URL.
- **F8 (asset)** — `GET /api/bff/assets/{id}/content` honors `If-None-Match` and
  answers `304 Not Modified`.
- **F9** — 500 responses return a generic `Internal server error`; the raw
  database/provider detail is logged server-side instead
  ([`map_domain_error`](../backend/src/adapter/driving/rest/handlers/mod.rs:65)).

Items **intentionally not changed** (re-audited against the actual consumers and
found to be deliberate contracts rather than flaws):

- **F4 / search slimming** — the search dialog renders `channel_count` and
  `bikes_last_day` per result row, so removing them would regress the UI; with
  the F3 batching the per-station stats are no longer an N+1.
- **F7 / custom range** — the frontend deliberately reuses the `graphs_day`
  HATEOAS link as the base for the `individual` `from`/`to` range (documented in
  [`stationDetail/types.ts`](../frontend/src/features/stationDetail/types.ts:109) and
  [`stationsSummary/types.ts`](../frontend/src/features/stationsSummary/types.ts:66)); the
  backend ignoring the path segment is that contract, not a bug.
- **F10 / hygiene** — `detail_url`, the `actions` map and the flattened search
  fields are consumed by the frontend (`StationOverview.tsx`, `useStationSearch`,
  `StationListItem`); removing them would require coordinated, product-changing
  frontend edits for little benefit.
- **F8 / JSON caching headers** — the frontend currently does not send `as_of`,
  so windowed JSON URLs are not pure functions of their URL and cannot be safely
  `max-age`-cached. ETag/304 on JSON would require clients to pin `as_of` first;
  deferred until that frontend change lands.

## Design decisions

| Concern | Decision |
|---|---|
| Bounds pushdown | Add `find_in_bounds(GeoBounds)` and `count_all()` to `CountingStationRepository` (SQL `WHERE latitude/longitude BETWEEN`); keep the existing `find_filtered` for the public REST API. |
| N+1 elimination | Add a single repository aggregate that returns the previous-local-day sum grouped by station for a given station set (one query for many stations), reusing the existing per-channel window aggregate internally. |
| Search | Make the search shell identity-only (id, name, description, image_url, coordinates) without `bikes_last_day`; keep `summaries` for the endpoints that actually show it. |
| `as_of`/`exclude_new_stations` | Extend `AsOfQueryParams` usage to the monthly and overview-stats sub-resources and emit matching HATEOAS links. |
| Custom range | Route "Individual" ranges to a dedicated `graphs/custom` path (or reject a mismatched `{timeframe}`); do not silently ignore the path segment. |
| Caching | Add `Cache-Control`/`Vary` to BFF JSON responses (short `max-age` for windowed, `no-store` for `Utc::now()`-based endpoints); honor `If-None-Match` on assets with `304`. |
| Error redaction | Split `map_domain_error` so 500 bodies return a generic message and the detail is logged server-side. |
| Hygiene | Drop `StationSearchDto.actions`, drop `detail_url` in favor of the frontend building the route from `id`, and slim `StationSummaryDto`. |

## Approach

### Task 1 — Repository pushdown (F1, F2)

- Extend [`CountingStationRepository`](../backend/src/core/domain/counting_stations/repository_port.rs:4)
  with `find_in_bounds(bounds: GeoBounds) -> Result<Vec<CountingStation>>` and
  `count_all() -> Result<usize>`.
- Implement both in [`PostgresCountingStationRepository`](../backend/src/adapter/driven/postgres/counting_station_repository.rs:1)
  with `WHERE latitude BETWEEN $1 AND $2 AND longitude BETWEEN $3 AND $4`
  (coordinates are nullable; positioned stations only) and `SELECT count(*)`.
- Update every in-memory test double of the trait.

### Task 2 — Use pushdown in BFF + analytics (F1, F2)

- [`list_bff_stations`](../backend/src/adapter/driving/bff/handlers.rs:228) and the
  sidebar/summary handlers call `station_analytics_service` with bounds; the service
  switches [`stations_for_bounds`](../backend/src/core/application/station_analytics/service.rs:248),
  [`included_stations`](../backend/src/core/application/station_analytics/service.rs:89) and
  [`summary_stations_in_bounds`](../backend/src/core/application/station_analytics/service.rs:109)
  to `find_in_bounds` instead of `find_filtered(None)` + in-memory filter.
- [`get_bff_stations_sidebar`](../backend/src/adapter/driving/bff/handlers.rs:287) uses
  `count_all()` for `total_count`.

### Task 3 — Batch per-station sums (F3)

- Add a `sum_previous_local_day_by_station(stations, channels_by_station, now)`
  batch helper (single query or one query per distinct timezone, grouped by station)
  to the measurement repository/service seam.
- Replace the per-station loop in
  [`bikes_by_station`](../backend/src/core/application/station_analytics/service.rs:287) and the
  per-station loop in [`global_summary`](../backend/src/core/application/station_analytics/service.rs:379).

### Task 4 — Slim the search endpoint (F4, F10)

- Introduce an identity-only search projection for
  [`get_bff_stations_search`](../backend/src/adapter/driving/bff/handlers.rs:371): name,
  description, id, image_url, coordinates, status. Remove `bikes_last_day` and
  `channel_count` from the search payload and drop `actions`.

### Task 5 — Consistency of windowed sub-resources (F5, F6)

- Accept `AsOfQueryParams` on
  [`get_bff_station_detail_monthly`](../backend/src/adapter/driving/bff/handlers.rs:584) and
  [`get_bff_station_overview_stats`](../backend/src/adapter/driving/bff/handlers.rs:690),
  honoring `as_of` and `exclude_new_stations`.
- Emit `as_of` on the monthly links in
  [`detail_page_links`](../backend/src/adapter/driving/bff/handlers.rs:418) and
  [`summary_page_links`](../backend/src/adapter/driving/bff/handlers.rs:707).

### Task 6 — Custom range route semantics (F7)

- Add `/api/bff/station-detail/{id}/graphs/custom` and
  `/api/bff/stations/summary/graphs/custom` for the Individual range, or validate
  that a supplied `{timeframe}` equals `custom`. Update the frontend
  [`stationDetail/api.ts`](../frontend/src/features/stationDetail/api.ts:1) and
  [`stationsSummary/api.ts`](../frontend/src/features/stationsSummary/api.ts:1) callers.

### Task 7 — Caching headers + conditional assets (F8)

- Add a small header helper to the BFF handlers; set `Cache-Control`/`Vary` on the
  windowed JSON sub-resources and `no-store` on `Utc::now()`-based endpoints.
- In [`get_bff_asset_content`](../backend/src/adapter/driving/bff/handlers.rs:938), parse
  `If-None-Match` and return `304` when it matches the asset ETag.

### Task 8 — Redact error details (F9)

- Change [`map_domain_error`](../backend/src/adapter/driving/rest/handlers/mod.rs:65) to log
  the `Database`/`Provider` detail server-side and return a generic
  `"Internal server error"` body (keeps `NotFound` and `InvalidQuery` as-is).
  Update the existing tests in [`mod.rs`](../backend/src/adapter/driving/rest/handlers/mod.rs:110).

### Task 9 — Hygiene (F10)

- Remove `detail_url` from [`StationOverviewDto`](../backend/src/adapter/driving/bff/dto.rs:193)
  (frontend builds `/stations/{id}` from `id`); remove `actions` from
  [`StationSearchDto`](../backend/src/adapter/driving/bff/dto.rs:170); slim
  [`StationSummaryDto`](../backend/src/adapter/driving/bff/dto.rs:36) to the fields the search
  dialog renders.

### Task 10 — Docs, tests, gates

- Update the utoipa annotations for every changed endpoint and regenerate the OpenAPI.
- Add/adjust BFF handler + analytics service tests for the new behavior.
- Run `make check`, `make test-rest`, `make test`, `make coverage`, and
  `make test-playwright` (frontend callers changed).

## Definition of done

- [ ] Plan registered in [`plans/README.md`](../plans/README.md:1)
- [ ] `make check` green
- [ ] `make test` / `make test-rest` green
- [ ] `make coverage` green
- [ ] `make test-playwright` green
- [ ] BFF endpoints no longer load full tables for viewport reads and no longer N+1 on stations
