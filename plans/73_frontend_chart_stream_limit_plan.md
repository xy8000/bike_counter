# 73 - Frontend chart data-stream limit

Status: implemented

## Problem

The station-summary page ([`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:182))
and the counting-station detail page
([`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:170)) both render
per-entity "Nerd stats" charts — one series/slice per station (summary) or per
channel (detail). When a view contains many stations or a station has many
channels, these charts become unreadable (many overlapping lines / radar webs /
donut slices). The chart palette in
[`chartUtils.ts`](../frontend/src/features/stationDetail/chartUtils.ts:4) even wraps
around after five colours, acknowledging the problem but not preventing it.

## Goal

Do not render any chart that would draw more than **5 data-streams** (stations
or channels). Instead, show a small info-note explaining that the chart cannot
be loaded because too many data-streams need to be rendered.

- The limit applies to **all** per-station/per-channel charts: the time-series
  line chart, the weekday radar, the hour radar and the share pie.
- The limit counts **actually rendered streams** — with the "Compare previous
  period" checkbox on, each station/channel contributes a current + a previous
  stream, so `3 channels + compare = 6 streams → hidden`.
- The aggregate charts in the "Detailed statistics" section (at most `current` +
  `previous` = 2 series) are unaffected by design, because the guard lives in the
  shared chart components and counts the series/slices passed in.

## Decisions

- Centralise the threshold as `MAX_DATA_STREAMS = 5` in
  [`chartUtils.ts`](../frontend/src/features/stationDetail/chartUtils.ts:4) so every
  chart shares the same number.
- Add one reusable [`ChartLimitNotice`](../frontend/src/features/stationDetail/ChartLimitNotice.tsx:1)
  info-note component (centered muted text, matching the look of
  [`ChartEmptyState`](../frontend/src/features/stationDetail/ChartEmptyState.tsx:6)) and
  render it in place of the chart.
- Put the guard inside the chart components themselves
  ([`TimeSeriesLineChart`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:47),
  [`WeekdayRadar`](../frontend/src/features/stationDetail/WeekdayRadar.tsx:33),
  [`HourRadar`](../frontend/src/features/stationDetail/HourRadar.tsx:26),
  [`SharePie`](../frontend/src/features/stationDetail/SharePie.tsx:23)). This is the
  single source of truth: both the detail and summary pages pass their series
  through these components, so both views get the behaviour with no per-view
  wiring.
- The line chart checks `visibleSeries.length` (empty series are already
  dropped); the radars check `series.length` (their callers only pass series
  that have data); the pie checks its filtered `data.length` (slices with
  `total > 0`).

## Changes

### [`frontend/src/features/stationDetail/chartUtils.ts`](../frontend/src/features/stationDetail/chartUtils.ts:4)

- Add `export const MAX_DATA_STREAMS = 5`.
- Update the `seriesColor` comment: the palette no longer needs to wrap around
  (charts with more than five streams are hidden), but keep the modulo so the
  helper stays safe.

### [`frontend/src/features/stationDetail/ChartLimitNotice.tsx`](../frontend/src/features/stationDetail/ChartLimitNotice.tsx:1) (new)

- A small component rendering the info-note text, defaulting to a message built
  from `MAX_DATA_STREAMS` (e.g. `This chart cannot be loaded — too many
  data-streams to render (max 5).`).
- Accepts an optional `className` so each chart can pass its usual aspect-ratio
  / spacing classes.

### [`frontend/src/features/stationDetail/TimeSeriesLineChart.tsx`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:60)

- After computing `visibleSeries`:
  - keep the existing empty-state return for `visibleSeries.length === 0`;
  - add a return of `<ChartLimitNotice className={cn('aspect-[20/9]', className)} />`
    when `visibleSeries.length > MAX_DATA_STREAMS`.

### [`frontend/src/features/stationDetail/WeekdayRadar.tsx`](../frontend/src/features/stationDetail/WeekdayRadar.tsx:33)

- Before the `hasData` empty check, return
  `<ChartLimitNotice className={cn('aspect-square', className)} />` when
  `series.length > MAX_DATA_STREAMS`.

### [`frontend/src/features/stationDetail/HourRadar.tsx`](../frontend/src/features/stationDetail/HourRadar.tsx:26)

- Same guard as the weekday radar: return the notice when
  `series.length > MAX_DATA_STREAMS`.

### [`frontend/src/features/stationDetail/SharePie.tsx`](../frontend/src/features/stationDetail/SharePie.tsx:23)

- After computing `data = slices.filter((slice) => slice.total > 0)`, extend the
  existing `data.length === 0` branch with a second branch: when
  `data.length > MAX_DATA_STREAMS`, render `<ChartLimitNotice className="py-16" />`
  instead of the donut + legend.

### [`frontend/e2e/e2e-seed.sql`](../frontend/e2e/e2e-seed.sql:1405)

- In the synthesized-measurements `WHERE` clause, add the Gasselstiege station
  (`97514fa2-2a21-4a17-b85c-6ec4aa74db27`, 6 channels) to the Münster list so a
  detail-page e2e test can deterministically hit the >5-channel path.

### [`frontend/e2e/detail.spec.ts`](../frontend/e2e/detail.spec.ts:69)

- Add a test that opens `/stations/97514fa2-2a21-4a17-b85c-6ec4aa74db27` and
  asserts the per-channel "This week by channel" card shows the info-note text
  (and contains no `.recharts-wrapper`), and that the "Share by channel" card
  shows the notice too.

### [`frontend/e2e/summary.spec.ts`](../frontend/e2e/summary.spec.ts:37)

- Add a test that opens `/summary` with a bounds spanning Münster + Bonn (the
  fixture synthesizes measurements for 4 Münster + 3 Bonn stations there = 7
  stations with data) and asserts the Nerd-stats per-station charts show the
  info-note text.

## Out of scope

- The summary page's per-station card currently reuses the detail page's
  `perChannelTitle` ("… by channel") even though it groups by station. This is a
  pre-existing label inconsistency and is not changed here.

## Verification

- Manual: a station with 6 channels shows the info-note in all four nerd-stats
  charts on the detail page; a summary view over >5 stations shows the note in
  the summary nerd-stats charts; toggling "Compare previous period" on a
  3-channel station hides the per-channel line chart (6 streams).
- Playwright e2e covers both the detail (>5 channels) and summary (>5 stations)
  paths.

## Gates

- `make check` green (includes frontend `prettier --check`).
- `make test-playwright` green.

## Definition of done

- [ ] `MAX_DATA_STREAMS` + `ChartLimitNotice` added.
- [ ] All four chart components guard against >5 streams.
- [ ] Fixture extended + detail/summary e2e tests added.
- [ ] [`plans/README.md`](../plans/README.md:1) updated.
- [ ] `make check` + `make test-playwright` green.
