# 45 - All-time bike counter on overview/detail/summary + drop latest-year trend

Status: completed

## Problem

Two related reporting gaps on the three station pages:

1. Every overview metric is a **period** stat (last 24 h / 7 days / month /
   year). There is no counter for the **all-time** total of bikes counted, so
   the reader cannot see a station's (or a view's) lifetime total.
2. The "Bikes per month" bar chart
   ([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:1))
   shows a year-over-year percentage + up/down arrow on **every** year button.
   The latest year is always incomplete (the current year, still being
   imported), so comparing its partial total against the previous full year is
   meaningless.

## Scope

- Backend: expose an all-time total (`total_bikes`) on the three page-shaped BFF
  payloads (overview, detail, summary). No data-model/migration changes.
- Frontend: render the all-time total on the overview panel, the detail page and
  the summary page, and suppress the year-over-year trend on the latest year
  button of the monthly bar chart (shared by the detail and summary pages).

Out of scope: the header global summary, the REST API, the other overview
metrics' trends, and the timeframe "compare previous period" charts.

## Decisions / assumptions

- **Semantics** (confirmed): `total_bikes` is the per-station all-time total on
  the overview panel and the detail page, and the **aggregated** all-time total
  of the included (non-disabled) stations on the summary page.
- **Computation**: reuse the existing
  [`MeasurementRepository::sum_by_month`](../backend/src/core/domain/measurements/repository_port.rs:121)
  (already implemented for Postgres and all in-memory mocks). The all-time total
  is the sum of the returned per-month totals, which avoids adding a new
  repository port method and is consistent with the monthly bar chart already on
  the detail/summary pages. For the summary page the total is derived from the
  already-computed `graphs.monthly_totals` (no extra query).
- **Placement/label**: a single shared component labelled `Total bikes (all
  time)` — a full-width highlighted card above the metric grid on the detail and
  summary pages, and a compact equivalent in the overview panel.
- **Latest-year trend**: the latest year is `years[years.length - 1]` in the
  sorted `years` array (the most recent year present in the data). Its
  `deltaPercent`/`trend` are forced to `null`, so it renders `–` with no arrow,
  exactly like the oldest year (which has no previous year).

## Backend changes

1. [`station_overview/mod.rs`](../backend/src/core/domain/station_overview/mod.rs:16)
   — add `pub total_bikes: i64` to `StationOverview`.
2. [`stations_summary/mod.rs`](../backend/src/core/domain/stations_summary/mod.rs:27)
   — add `pub total_bikes: i64` to `StationsSummary`.
3. [`station_overview_service.rs`](../backend/src/core/application/station_overview_service.rs:70)
   — in `overview()`, after the four metric windows compute the all-time total
   via `sum_by_month(&station.timezone.0, &channel_ids)` and sum the month
   totals; include it in the returned `StationOverview`.
4. [`stations_summary_service.rs`](../backend/src/core/application/stations_summary_service.rs:456)
   — in `summarize()`, after `let graphs = ...`, compute
   `total_bikes` as the sum of `graphs.monthly_totals` and include it in the
   returned `StationsSummary` (it already reflects only the included stations).
5. [`dto.rs`](../backend/src/adapter/driving/bff/dto.rs:111)
   — add `pub total_bikes: i64` to `StationOverviewDto`, `StationDetailDto` and
   `StationsSummaryPageDto`; update the `From<StationsSummary>` impl.
6. [`handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:292)
   — set `total_bikes: overview.total_bikes` in the overview and detail
   responses (the summary response picks it up via the `From` impl).

## Frontend changes

7. [`stationOverview/types.ts`](../frontend/src/features/stationOverview/types.ts:14)
   — add `total_bikes: number` to `StationOverview`.
8. [`stationDetail/types.ts`](../frontend/src/features/stationDetail/types.ts:70)
   — add `total_bikes: number` to `StationDetail`.
9. [`stationsSummary/types.ts`](../frontend/src/features/stationsSummary/types.ts:52)
   — add `total_bikes: number` to `StationsSummary`.
10. New shared component
    [`TotalBikesCard.tsx`](../frontend/src/features/stationOverview/TotalBikesCard.tsx:1)
    — renders the label `Total bikes (all time)` plus
    [`formatNumber`](../frontend/src/lib/format.ts:7) and the muted `bikes`
    suffix (same visual language as
    [`MetricCard.tsx`](../frontend/src/features/stationOverview/MetricCard.tsx:17)).
11. [`StationOverview.tsx`](../frontend/src/features/stationOverview/StationOverview.tsx:75)
    — render the counter (compact) above the metric list.
12. [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:198)
    — render the counter in the `Overview` section above the metric grid.
13. [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:262)
    — render the counter in the `Overview` section above the metric grid.
14. [`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:83)
    — in the `yearlyTotals` memo, skip the trend/percentage computation for the
    latest year so its button renders `–` with no arrow (oldest year unchanged).

## Testing / gates

- Backend unit tests: extend
  [`station_overview_service.rs`](../backend/src/core/application/station_overview_service.rs:149)
  (implement the in-memory `sum_by_month` so `total_bikes` is meaningful, and
  assert it) and
  [`stations_summary_service.rs`](../backend/src/core/application/stations_summary_service.rs:531)
  (assert `total_bikes`).
- BFF tests: extend
  [`rest/tests/bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:337)
  to assert `total_bikes` is present in the overview and summary payloads.
- Frontend e2e: extend
  [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:123) and
  [`summary.spec.ts`](../frontend/e2e/summary.spec.ts:58) to assert the
  `Total bikes (all time)` counter; assert the **last** monthly-bar year button
  shows `–` and no `%` (the latest-year trend is gone).
- Run the gates: `make check`, `make test-rest`/`make test`, `make coverage`,
  `make frontend-build`, `make test-playwright` (see
  [`agents.md`](../agents.md:25)).

## Definition of done

- [x] Plan registered in [`plans/README.md`](../plans/README.md:1)
- [x] `total_bikes` exposed on overview/detail/summary BFF payloads
- [x] All-time counter rendered on the overview panel, detail page and summary
      page
- [x] Latest-year trend removed from the monthly bar chart
- [x] Backend + frontend tests updated and green
- [x] `make check`, `make test-rest`, `make coverage`, `make frontend-build` and
      `make test-playwright` green
