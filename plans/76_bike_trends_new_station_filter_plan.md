# 76 - Bike-Trends: exclude stations without full-period coverage

Status: implemented

## Follow-up (review feedback)

- **Monthly bar chart**: with the setting on, the summary page's `Bikes per
  month` chart now also drops stations that lack full data for the whole current
  + previous year (the same windows as the `Year` timeframe). The detail page's
  monthly chart stays factual — a single station's own history is not skewed by
  "new" stations.
- **Settings placement**: the gear button is removed from the global header;
  a small "Calculation settings" button opens the same `SettingsDialog` from the
  detail and summary pages instead.
- **Open-window coverage (bugfix)**: the "whole window" check used to require a
  measurement within one resolution interval of the window's end even for the
  still-running current week/year window (whose end is the reference `now`).
  Once the seeded/imported data lagged `now` by more than one interval, every
  established station failed and all summary graphs came back empty. A running
  window (`to >= now`) is now never a gating criterion — its data may not have
  arrived at all (incomplete week, import lag, a stale seed), so every station
  qualifies for it; only the completed windows (previous week/year, yesterday,
  last 30 days, last month) require data from their start through their end,
  which is exactly what excludes newly-built stations (they have no
  previous-period data). See [`covers_whole_window`](../backend/src/core/application/station_analytics/resolution.rs:56).

## Problem

When a counting station is newly built (or has gaps at the edges of the compared
period), its measurements only cover part of the window. The trend metrics
(up/down/delta) and the current-vs-previous graph overlays then compare a
"current" total that includes that station against a "previous" total that does
not, producing a misleading up-trend. There is no way to opt out of this
behaviour.

## Goal

Add a small settings dialogue that toggles "exclude new stations from trends".
When enabled, a station is included in a trend/metric or comparison graph only
if it has data covering the **whole** relevant window (and, when the previous
period is part of the comparison, the **whole** previous window as well). The
check is written generically over any `(from, to)` window so a future custom
from/to date picker reuses it unchanged.

The setting applies to:

- the station-summary page (aggregate overview metrics + graphs/nerd stats),
- the station-detail page (overview metrics show "New" instead of a misleading
  trend; graphs flag a missing baseline),
- the global header summary (bikes last-day total over fully-covered stations).

## Decisions

- **Coverage rule**: a station "has data for the whole window" when, at some
  resolution, its first measurement is within one interval of `from` and its
  last within one interval of `to`. This reuses the existing
  [`covers()`](../backend/src/core/application/station_analytics/resolution.rs:40)
  tolerance logic, exposed as `covers_whole_window` and the public
  `has_full_coverage(coverage, from, to, now)`. A still-running window (its `to`
  is the reference `now`, e.g. the current week/year) is open-ended and **never
  gates**: data for it may not have arrived yet, so every station qualifies and
  only the completed (previous) window is decisive — which is what drops
  newly-built stations, since they have no previous-period data (see the bugfix
  in the follow-up section).
- **Inclusion rule**: with the setting on, for each timeframe/metric a station is
  included only if it has full coverage of the **previous** window, plus of the
  **current** window when the current period has already ended (yesterday, last
  30 days, last month). A still-running current window (this week, this year)
  never gates: its data may not have arrived yet, so every station qualifies and
  only the completed baseline is decisive — that is what drops newly-built
  stations (they have no previous-period data). The same station set feeds the
  current and previous series, so the trend and the compare overlay are always
  like-for-like over the completed baseline.
- **Generic seam**: the predicate only needs `(coverage, from, to)`; all window
  math is already centralized in
  [`graphs::graph_windows`](../backend/src/core/application/station_analytics/graphs.rs:84)
  and [`metrics::metric_windows`](../backend/src/core/application/station_analytics/metrics.rs:51).
  A future date picker changes only `from`/`to` and the filter keeps working.
- **Per-station coverage without N+1**: a new
  `MeasurementRepository::resolution_coverage_by_channel` returns per-channel
  coverage (channel id + resolution + first/last/count) in one query per window;
  the summary service maps channel → station to build the included set. The
  detail page and global summary use the existing aggregate
  `resolution_coverage` over one station's channels (detail) or one query per
  station in the already-existing per-station global loop.
- **Single-station detail page is never filtered**: there is nothing to compare
  against, so the page instead reports `is_new` (true when the station lacks full
  coverage of the metric/timeframe windows) and the UI presents it honestly.
- **Persistence**: the setting is stored in `localStorage` and shared through a
  React context; summary/detail/header requests append `exclude_new_stations=true`
  (mirroring the existing `exclude` query-param pattern).
- **Non-trend data is untouched**: `channel_count` and `total_bikes` are factual
  totals and are not filtered. The summary page's monthly bar chart is the one
  exception added in review: with the setting on it drops stations that lack full
  data for the whole current + previous year (the same windows as the `Year`
  timeframe), so the yearly grouping does not change depending on the slider. The
  detail page's monthly chart stays factual — a single station's own history is
  not skewed by "new" stations.

## Changes

### Backend

- [`mod.rs`](../backend/src/core/domain/station_analytics/mod.rs:89): add `is_new`
  to `MetricWindow` and `PeriodGraphs`.
- [`repository_port.rs`](../backend/src/core/domain/measurements/repository_port.rs:65):
  add `ChannelCoverage` and
  `resolution_coverage_by_channel(from, to, channel_ids) -> Vec<ChannelCoverage>`;
  implement it in
  [`measurement_repository.rs`](../backend/src/adapter/driven/postgres/measurement_repository.rs:526)
  (one `GROUP BY channel_id, resolution_seconds` query) and in the in-memory
  mock in [`tests.rs`](../backend/src/core/application/station_analytics/tests.rs:410).
- [`resolution.rs`](../backend/src/core/application/station_analytics/resolution.rs:40):
  expose `has_full_coverage(coverage, from, to)` = any resolution covers the
  window.
- [`metrics.rs`](../backend/src/core/application/station_analytics/metrics.rs:51):
  `metric_windows` learns `exclude_new_stations`. Per station it queries one
  aggregate `resolution_coverage` over the union of all eight metric windows, then
  for each metric includes the station only when it fully covers that metric's
  current **and** previous window (multi-station summary), or sets `is_new` when
  it does not (single-station detail, keeping the totals).
- [`graphs.rs`](../backend/src/core/application/station_analytics/graphs.rs:187):
  `period_data` / `period_graphs_per_station` learn `exclude_new_stations`; when
  on, they query `resolution_coverage_by_channel` for the current and previous
  windows, derive the fully-covered station set, and fold only that set into the
  aggregate series, weekday/hour radars, pie and per-station series.
  `period_graphs_per_channel` (detail) sets `PeriodGraphs.is_new` from the
  station's coverage of the current + previous windows.
- [`service_port.rs`](../backend/src/core/domain/station_analytics/service_port.rs:16)
  and [`service.rs`](../backend/src/core/application/station_analytics/service.rs:247):
  add the flag to `detail_overview_stats`, `detail_graphs_timeframe`,
  `stations_summary_overview`, `stations_summary_graphs_timeframe` and
  `global_summary`. The global summary filters `bikes_last_day_total` to stations
  with full coverage of their last-day current + previous comparison windows.
  `stations_summary_monthly` (added in review) drops stations without full current
  + previous calendar-year coverage when the flag is on, through an
  `established_stations_for_year` helper that reuses the per-channel coverage
  query over the union window `[previous-year start, now]`.
- [`dto.rs`](../backend/src/adapter/driving/bff/dto.rs:244): add
  `exclude_new_stations` to `AsOfQueryParams` and `BffStationSummaryQueryParams`,
  add `GlobalSummaryQueryParams`, and expose `is_new` on `MetricDto` and
  `PeriodGraphsDto` (with the `From` impls).
- [`handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:482): pass the
  flag through, accept it on `GET /api/bff/global-summary`, and update
  [`openapi.rs`](../backend/src/adapter/driving/rest/openapi.rs:1) registrations.
- Tests: unit tests in [`tests.rs`](../backend/src/core/application/station_analytics/tests.rs:1)
  and REST tests in [`bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:1)
  for full-coverage filtering, `is_new` and the new per-channel coverage method.

### Frontend

- New [`features/settings/`](../frontend/src/features/settings): a
  `TrendSettingsProvider` + `useTrendSettings` hook (localStorage-backed) and a
  `SettingsDialog` (checkbox) built on the shadcn
  [`Dialog`](../frontend/src/components/ui/dialog.tsx:7) primitives.
- [`App.tsx`](../frontend/src/App.tsx:9): wrap the routes in the provider.
- The header's [`TopBar.tsx`](../frontend/src/features/header/TopBar.tsx:11) keeps
  the global summary, which reads the flag via
  [`useGlobalSummary.ts`](../frontend/src/features/header/useGlobalSummary.ts:6)
  and refetches on change; no settings gear lives in the header.
- A small "Calculation settings" button next to the detail-page and summary-page
  timeframe controls opens the `SettingsDialog` (backed by a local `settingsOpen`
  state in [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:240)
  and [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:289)).
- Summary page ([`api.ts`](../frontend/src/features/stationsSummary/api.ts:41),
  [`useStationsSummaryOverview.ts`](../frontend/src/features/stationsSummary/useStationsSummaryOverview.ts:8),
  [`useStationsSummaryGraphs.ts`](../frontend/src/features/stationsSummary/useStationsSummaryGraphs.ts:9)):
  append the flag alongside `exclude` and include it in the dependency keys.
- Detail page ([`api.ts`](../frontend/src/features/stationDetail/api.ts:37),
  [`useStationOverviewStats.ts`](../frontend/src/features/stationDetail/useStationOverviewStats.ts:6),
  [`useStationGraphs.ts`](../frontend/src/features/stationDetail/useStationGraphs.ts:7)):
  append the flag; [`MetricCard.tsx`](../frontend/src/features/stationOverview/MetricCard.tsx:17)
  renders a neutral "New" indicator when `is_new`, and
  [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:239)
  shows a "no full-period data to compare" notice for `is_new` graphs.
- Types: add `is_new` to
  [`StationOverviewMetric`](../frontend/src/features/stationOverview/types.ts:5)
  and [`PeriodGraphs`](../frontend/src/features/stationDetail/types.ts:58).
- Playwright: extend the summary/detail specs with a settings-toggle scenario.

### Docs / gates

- Register this plan in [`plans/README.md`](../plans/README.md:13).
- `make check`, `make test-rest` / `make test`, `make coverage`,
  `make test-playwright` all green.
