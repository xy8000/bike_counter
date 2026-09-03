# 97 - Time-series line charts become bar charts

Status: implemented (make check + make test-playwright green)

## Problem

The app renders every time-series trend chart as a line chart via
[`TimeSeriesLineChart.tsx`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:49).
These charts appear in four places:

- the aggregate trend (current period, plus the previous period when compare is
  on) on the detail page and the stations-summary page,
- the per-channel trend on the detail page,
- the per-station trend on the stations-summary page.

The request is to show all of them as bar charts instead of line charts.

## Goal / Decisions

1. Convert the single line-chart component to a bar chart. The radar charts
   (`WeekdayRadar`, `HourRadar`), the share donut (`SharePie`/`ChannelPie`) and
   the existing `MonthlyBarChart` are not line charts and stay unchanged.
2. Layout semantics (confirmed with the user):
   - **Current vs previous period** → side-by-side bars.
   - **Multiple channels / stations in the same comparison** → stacked into one
     bar; the previous period forms a second, separate side-by-side bar.
3. This is expressed generically with a per-series `stackId`: series that share
   a `stackId` stack together, different `stackId`s sit side-by-side.
   - `timeframeSeries` (aggregate): `current` and `previous` each get their own
     `stackId` → one bar per period (side-by-side).
   - `channelSeries` / `stationSeries`: all `_current` series share the
     `stackId` `current`, all `_previous` series share `previous` → channels
     stack within the current bar, the previous period is its own bar.
4. The x-axis becomes categorical (`type="category"`, tick labels still formatted
   with the existing `xFormatter`). The previous numeric `scale="time"` + fixed
   `xDomain` extension (leftover days of a running window) is dropped: a bar
   chart only draws buckets that have data, which is visually clear without
   fabricating empty trailing slots.

## Approach

### Task 1 — Rename + convert the chart component

Rename
[`TimeSeriesLineChart.tsx`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:1)
to `TimeSeriesBarChart.tsx` and:

- swap the recharts import from `Line, LineChart` to `Bar, BarChart`,
- rename `LineSeries` → `BarSeries` and add a `stackId: string` field,
- rename the exported component to `TimeSeriesBarChart`,
- drop the `xDomain` prop,
- replace the `<LineChart>` with `<BarChart accessibilityLayer
  margin={{ left: 12, right: 12 }}>` (matching `MonthlyBarChart`),
- change the `<XAxis>` to `type="category"`, keep `dataKey="time"` +
  `tickFormatter={xFormatter}` + `minTickGap`, and remove `scale="time"` and
  `domain`,
- replace each `<Line … />` with
  `<Bar dataKey={item.key} stackId={item.stackId} fill={var(--color-<key>)}
  radius={2} isAnimationActive={false} />`,
- keep the tooltip `labelFormatter` unchanged (it reads the numeric
  `payload.time` and formats it through the existing `tooltipFormatter` /
  `xFormatter`),
- keep the `ChartEmptyState` / `ChartLimitNotice` guards and the legend logic
  unchanged,
- update the file's doc comments ("line chart" → "bar chart").

### Task 2 — Set `stackId` on every series

- [`timeframes.ts`](../frontend/src/features/stationDetail/timeframes.ts:188)
  `timeframeSeries`: add `stackId: 'current'` to the current series and
  `stackId: 'previous'` to the previous series.
- [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:51)
  `channelSeries`: add `stackId: 'current'` / `stackId: 'previous'` to the
  `${channel_id}_current` / `${channel_id}_previous` series.
- [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:52)
  `stationSeries`: same for `${station_id}_current` / `${station_id}_previous`.
- Update the `BarSeries[]` type annotations in these three files.

### Task 3 — Remove the dead domain code

With `xDomain` gone, `timeframeDomain` is no longer called:

- Remove the `timeframeDomain` import + the `domain` variable in
  [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:24)
  and [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:25)
  and delete the `xDomain={domain}` props. Keep `anchor` — `alignSeries` still
  needs it.
- Remove the now-unused `timeframeDomain` function and the `domainWidthMs`
  field/values from [`timeframes.ts`](../frontend/src/features/stationDetail/timeframes.ts:61)
  (plus the `DAY_MS`/`HOUR_MS` locals that only feed `domainWidthMs`) so
  `tsc` stays green under `noUnusedLocals`.

### Task 4 — Fix stale wording

Update comments/docs that still describe the trend chart as a line chart:

- [`ChartEmptyState.tsx`](../frontend/src/features/stationDetail/ChartEmptyState.tsx:4)
  ("across the line chart …").
- [`Skeletons.tsx`](../frontend/src/features/stationDetail/Skeletons.tsx:72)
  ("full-width line-chart card").
- Section comments in
  [`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:347)
  and [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:418)
  ("full-width line chart" → "bar chart"; "one line per channel/station").
- The e2e comment/variable `lineCard` in
  [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:159) → `barCard` (cosmetic;
  no assertion changes needed — the specs only assert `.recharts-wrapper`).

### Task 5 — Gates

- `make check` (prettier + TypeScript build pass).
- `make test-playwright` (frontend UI change; no backend change expected, so
  `make test-rest` / full `make test` are only needed if nothing else regresses).

## Notes

- `barGap` / `barCategoryGap` are left at their defaults initially; they can be
  tuned later if the side-by-side period bars look cramped on dense buckets
  (e.g. the 5-minute "24 hours" view).
- The tooltip and legend reuse the existing `chart.tsx` helpers, so no changes
  are needed there.
