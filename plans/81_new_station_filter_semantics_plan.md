# 81 - New-station filter: only exclude stations introduced in the timeframe

Status: implemented

## Implementation notes

All backend and frontend changes are in place. The repository gained
`earliest_by_channel`, the retired `covers_whole_window`/`has_full_coverage`
predicates were replaced by `introduced_after`, all five analytics call sites
(filtering + `is_new`) were migrated, the existing tests were updated to the new
semantics and regression tests for the data-loss case were added.

All gates are green: `make check`, `make test` (486 tests incl. Postgres),
`make coverage` (overall 87.34%, core 95.23%) and `make test-playwright`
(48 e2e tests).

## Problem

The Bike-Trends "Exclude new stations" setting (query parameter
`exclude_new_stations=1`) currently keeps a station only when it has
measurements covering the **whole** comparison window — both its start and its
end (see [`covers_whole_window`](../backend/src/core/application/station_analytics/resolution.rs:74)).

That is too strict. A station with a brief data loss or outage (a few missing
data points, or an import that lags a day) fails the "data through the end"
check and is dropped, even though the station existed for the entire period.
On a real stack this can empty the whole summary: with `timeframe=year&compare=1`
a station must fully cover the previous calendar year (and, for the overview's
"last year" metric, the year before it), so any gap at the edges removes every
station and the page reports "no data".

The user expectation is different: **only stations that were introduced inside
the selected timeframe should be ignored** — stations that existed before the
period but had data loss must stay.

## Root cause

The predicate used everywhere by the filter is
[`covers_whole_window`](../backend/src/core/application/station_analytics/resolution.rs:74):

```rust
first <= from + r && last >= to - r
```

`first`/`last` are the earliest/latest measurement **within** the queried
window, so the predicate effectively requires no gap at either boundary. The
call sites are:

- [`metrics::metric_windows`](../backend/src/core/application/station_analytics/metrics.rs:80)
  (summary overview + detail overview via `has_full_coverage`),
- [`graphs::period_data`](../backend/src/core/application/station_analytics/graphs.rs:412)
  (summary + detail graph aggregation via `covers_whole_window`),
- [`graphs::period_graphs_per_channel`](../backend/src/core/application/station_analytics/graphs.rs:771)
  (detail `is_new` flag via `has_full_coverage`),
- [`service::global_summary`](../backend/src/core/application/station_analytics/service.rs:379)
  (header last-day total via `has_full_coverage`),
- [`service::established_stations_for_year`](../backend/src/core/application/station_analytics/service.rs:194)
  (summary monthly chart via `covers_whole_window`).

## Goal

Change the filter to a single, uniform rule: a station/group is excluded as
"new" **iff it was introduced at or after the start of the comparison window** —
that is, its **earliest-ever measurement is not before the window start**. Data
loss, outages and import lag inside the window no longer exclude a station.

For a (current, previous) comparison the decisive start is the **previous**
window's start (a station that existed before the previous window also existed
before the current one, so the current window never needs to gate). A custom
"Individual" range has no previous window, so its own start is used.

## New semantics (formal)

- Let `earliest` be the station's first-ever measurement timestamp (the MIN of
  `MIN(timestamp)` over its channels, at any resolution).
- For each comparison window pair `(current, previous)` (or just `current` for a
  custom range), the reference start is `reference_from = previous.from`
  (or `current.from`).
- `is_new = earliest >= reference_from`.
- Multi-station aggregates (summary page) drop `is_new` stations for that
  metric/timeframe; the detail page keeps totals but sets `is_new` so the UI can
  show a neutral indicator instead of a misleading trend.

This replaces the "full coverage of start AND end" rule and subsumes the earlier
"running window never gates" special case: we only compare the earliest
timestamp against the previous window's start, which is always a completed
window for the fixed timeframes.

## Decisions

- Add a single repository method `earliest_by_channel(channel_ids) ->
  Vec<ChannelFirst>` (MIN timestamp per channel, no lower bound), mirroring the
  existing `latest_by_channel`. One query feeds all filter decisions.
- Replace `covers_whole_window`/`has_full_coverage` with a tiny
  `introduced_after(earliest, from) -> bool` predicate in
  [`resolution.rs`](../backend/src/core/application/station_analytics/resolution.rs).
  The unrelated `covers`/`covers_window` helpers used by `select_resolution`
  stay untouched.
- Keep the comparison strict (`earliest >= from` means "new"); no per-resolution
  tolerance is needed because `earliest` is a global minimum rather than an
  in-window minimum.
- The detail page stays "never filtered, only flagged": `is_new` derives from the
  same predicate.

## Changes

### Backend

1. [`repository_port.rs`](../backend/src/core/domain/measurements/repository_port.rs:123):
   add `ChannelFirst { channel_id, timestamp }` and
   `earliest_by_channel(channel_ids) -> Vec<ChannelFirst>`, defaulting to empty.
2. [`measurement_repository.rs`](../backend/src/adapter/driven/postgres/measurement_repository.rs:707):
   implement `earliest_by_channel` as
   `SELECT channel_id, MIN(timestamp) FROM measurements WHERE channel_id = ANY($1) GROUP BY channel_id`.
   Implement it in the in-memory mock in
   [`tests.rs`](../backend/src/core/application/station_analytics/tests.rs:410)
   and any REST test double.
3. [`resolution.rs`](../backend/src/core/application/station_analytics/resolution.rs:56):
   add `introduced_after(earliest, from)`; remove `covers_whole_window` and
   `has_full_coverage` once their call sites are migrated (and update/remove their
   tests).
4. [`metrics.rs`](../backend/src/core/application/station_analytics/metrics.rs:80):
   per station query `earliest_by_channel` over its channels, then for each metric
   compare `earliest` to that metric's previous-window `from` (`before_day_from`,
   `before_week_from`, `before_month_from`, `before_year_from`); multi-station
   skips, single-station sets `is_new`. Drop the union `resolution_coverage`
   query.
5. [`graphs.rs`](../backend/src/core/application/station_analytics/graphs.rs:412):
   in `period_data`, query `earliest_by_channel(channel_ids)` once, build the
   group earliest (min over the group's channels), and set
   `established_groups = { group : earliest < reference_from }` where
   `reference_from = previous.from` (or `current.from` for a custom range).
   Remove the two `resolution_coverage_by_channel` queries and the
   `covers_current`/`covers_previous` set logic.
6. [`graphs.rs`](../backend/src/core/application/station_analytics/graphs.rs:771):
   in `period_graphs_per_channel`, set `is_new = introduced_after(earliest,
   reference_from)` using the station's earliest.
7. [`service.rs`](../backend/src/core/application/station_analytics/service.rs:379):
   in `global_summary`, skip a station when `earliest >= before_from` (the start
   of the comparison day, i.e. two local days back).
8. [`service.rs`](../backend/src/core/application/station_analytics/service.rs:194):
   in `established_stations_for_year`, keep a station when `earliest <
   last_year_from` (start of the previous calendar year).

### Tests

- Update the existing Bike-Trends tests in
  [`tests.rs`](../backend/src/core/application/station_analytics/tests.rs:1877)
  to seed an "established" station with a first measurement **before** the
  window start (the current fixtures place it exactly at the start).
- Add regression tests:
  - an established station with a gap near the window end (data loss) is kept;
  - an established station with a stale latest measurement is kept (supersedes
    `established_stations_with_a_stale_last_measurement_are_not_dropped`);
  - a station introduced mid-window is dropped;
  - a station introduced exactly at the window start is dropped.
- Keep the REST assertions in
  [`bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:305) green.

### Frontend copy (match the new semantics)

- [`SettingsDialog.tsx`](../frontend/src/features/settings/SettingsDialog.tsx:156):
  replace "data covering the whole current ... period" with wording about
  excluding stations that opened during the compared period.
- [`TrendSettingsIllustration.tsx`](../frontend/src/features/settings/TrendSettingsIllustration.tsx:74):
  rephrase the off-state caption "a new station opens and adds bikes — sudden
  increase" to e.g. "a station opens partway through the period — totals jump",
  and the "only stations with a full year of data" line to
  "only stations open since before the period".
- [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:376):
  reword the `is_new` notice from "no data covering the whole compared period"
  to "opened during the compared period".
- [`MetricCard.tsx`](../frontend/src/features/stationOverview/MetricCard.tsx:29):
  update the "New" tooltip/comment to the new wording.
- [`TrendSettingsContext.tsx`](../frontend/src/features/settings/TrendSettingsContext.tsx:8):
  update the doc comment describing the flag.

### Docs / gates

- Register this plan in [`plans/README.md`](../plans/README.md).
- `make check`, `make test-rest` / `make test`, `make coverage`,
  `make test-playwright` all green.
