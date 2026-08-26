# 53 - Station-overview shell + stats sub-resource (name loads directly)

Status: implemented

## Problem

The overview panel (the left panel that replaces the sidebar when a map marker
is clicked) still loads **at once**: [`GET /api/bff/station-overview/{id}`](backend/src/adapter/driving/bff/handlers.rs:587)
returns the station identity **and** the expensive aggregates (`total_bikes` +
the four `metrics` windows) in one flat payload. The frontend
[`useStationOverview`](frontend/src/features/stationOverview/useStationOverview.ts:6)
fetches that single payload, so the panel shows only a skeleton — including a
placeholder heading — until the whole aggregation is computed. The station
**name** (and image/description) should render as soon as the identity arrives,
with the stats filling in afterwards, exactly like the sidebar split in plan 52.

## Goals

- Split `GET /api/bff/station-overview/{id}` into:
  - a **shell** (identity + `channel_count` + `last_update` + HATEOAS `stats`
    link) that loads directly, and
  - a **stats** sub-resource returning `total_bikes` + `metrics`.
- Reuse the existing [`detail_overview_stats`](backend/src/core/domain/station_analytics/service_port.rs:58)
  service method for the stats sub-resource (it already computes exactly
  `StationOverviewStats { total_bikes, metrics }`) — no duplicated metric work.
- Render the overview panel's header name/image/description from the shell
  immediately and show a `Skeleton` for the stats card until the stats arrive.
- Keep the measurement domain, `MeasurementRepository` and the public `/api/v1`
  API untouched.

## Non-goals

- No change to the detail page, summary page or sidebar endpoints (they already
  follow this pattern).
- No change to the map markers or the search dialog.

## Endpoint surface (all under the `BFF API` tag)

| Method | Path | Purpose | Response |
|---|---|---|---|
| GET | `/api/bff/station-overview/{id}` | overview shell | `StationOverviewDto` (shell) |
| GET | `/api/bff/station-overview/{id}/stats` | overview stats | `StationOverviewStatsDto` |

The shell carries `_links.stats = /api/bff/station-overview/{id}/stats` (a
concrete URL). The stats sub-resource uses server `now` (like the current
overview call) and returns the already-existing `StationOverviewStatsDto`
(`total_bikes` + `metrics`).

## Backend changes

### 1. Domain ([`station_analytics/mod.rs`](backend/src/core/domain/station_analytics/mod.rs:81))

Replace [`StationOverview`](backend/src/core/domain/station_analytics/mod.rs:82)
with:

```rust
pub struct StationOverviewShell {
    pub station: CountingStation,
    pub channel_count: usize,
    pub last_update: Option<DateTime<Utc>>,
}
```

[`StationOverviewStats`](backend/src/core/domain/station_analytics/mod.rs) stays
(already the detail overview card type). No other model changes.

### 2. Port ([`service_port.rs`](backend/src/core/domain/station_analytics/service_port.rs:46))

Replace `overview(station_id, now) -> StationOverview` with:

```rust
fn overview_shell(&self, station_id: Id) -> Result<StationOverviewShell, DomainError>;
```

The stats half reuses `detail_overview_stats(station_id, now) -> StationOverviewStats`
(unchanged).

### 3. Service ([`service.rs`](backend/src/core/application/station_analytics/service.rs:331))

- `overview_shell`: fetch the station + its channels (for the count) +
  `last_update` — **no** `metric_windows` / `sum_by_month` work.
- Delete the old `overview()` method.

### 4. BFF DTOs ([`bff/dto.rs`](backend/src/adapter/driving/bff/dto.rs:157))

- Reshape [`StationOverviewDto`](backend/src/adapter/driving/bff/dto.rs:158) into
  the shell: drop `total_bikes` + `metrics`, add `_links` (a `HashMap<String,
  LinkDto>` with `self` + `stats`). Keep `detail_url`.
- Reuse the existing `StationOverviewStatsDto` for the stats sub-resource.

### 5. BFF handlers ([`handlers.rs`](backend/src/adapter/driving/bff/handlers.rs:587))

- Rewrite `get_bff_station_overview` to resolve the shell
  (`overview_shell`), resolve the image URL and build `_links.stats`.
- Add `get_bff_station_overview_stats`: parse the id path, `now = Utc::now()`,
  call `detail_overview_stats`, return `StationOverviewStatsDto`.

### 6. Router + OpenAPI

- [`rest/mod.rs`](backend/src/adapter/driving/rest/mod.rs:94): add
  `/api/bff/station-overview/:id/stats`.
- [`openapi.rs`](backend/src/adapter/driving/rest/openapi.rs): register the new
  path/handler; `StationOverviewDto` and `StationOverviewStatsDto` schemas stay.

### 7. Tests

- [`station_analytics/tests.rs`](backend/src/core/application/station_analytics/tests.rs:687):
  convert the `overview_*` cases to `overview_shell` (channel count, last
  update, 404 on unknown) and point the metrics/timezone assertions at the
  existing `detail_overview_stats` cases (already covered).
- [`bff.rs`](backend/src/adapter/driving/rest/tests/bff.rs:366): update the
  overview test to the shell shape (`_links.stats` present, no `total_bikes` /
  `metrics`); add a stats endpoint test (`total_bikes` + four metrics, 404 on
  unknown station).

## Frontend changes

### 1. Types + API ([`stationOverview/`](frontend/src/features/stationOverview))

- `types.ts`: rename the combined type to `StationOverviewPage` (shell:
  identity + `channel_count` + `image_url` + `last_update` + `detail_url` +
  `_links.stats`) and add `StationOverviewStats { total_bikes, metrics }`.
- `api.ts`: `fetchStationOverview(id)` → shell (unwrap `_links.stats.href`);
  `fetchStationOverviewStats(url)` → stats.

### 2. Hook ([`useStationOverview.ts`](frontend/src/features/stationOverview/useStationOverview.ts))

Fetch the shell, then the stats sub-resource from its link. Return
`{ page, stats, error, statsError }` with independent loading/error states.

### 3. Component ([`StationOverview.tsx`](frontend/src/features/stationOverview/StationOverview.tsx))

- Render the header name, image, description, channel-count badge and
  last-update from `page` immediately (fallback "Counting station" heading only
  while the shell is in flight).
- Render `TotalBikesCard` + the `MetricCard` list from `stats`, showing a
  `Skeleton` (reuse the existing skeleton style) until they arrive, with a
  small error state on `statsError`.

## E2E changes

- [`map.spec.ts`](frontend/e2e/map.spec.ts): the overview assertions already
  wait for the loaded content (name, counter, metrics) via Playwright
  auto-wait; optionally add one assertion that the header name appears before
  the stats. No required changes.

## Definition of done

- [ ] Overview shell returns identity + `_links.stats`; the stats sub-resource
      returns `total_bikes` + `metrics`; the panel renders the name immediately.
- [ ] `MeasurementRepository` and the measurement domain unchanged; `detail` /
      `summary` / `sidebar` endpoints unchanged.
- [ ] `make check`, `make test-rest`, `make test`, `make coverage` green
      (core ≥ 95%).
- [ ] `make frontend-build` green.
- [ ] `make test-playwright` green.
- [ ] Plan registered in [`plans/README.md`](plans/README.md).
