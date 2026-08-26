# 42 - Frontend: monthly-bar grouping by year, full-width Nerd-stats chart and search-dialog fixes

Status: completed

## Problem

Three frontend issues reported on the detail / summary pages and the search
dialog:

1. **"Bikes per month" is not actually grouped by year.** The card description
   says "All available months, grouped by year", but
   [`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:1)
   renders one flat bar per calendar month across all years (x-axis like
   "Jan 2024" … "Dec 2025") and only a grand total in the top-right corner. It
   should follow the shadcn **interactive bar-chart** pattern with each **year**
   as a clickable header button (showing that year's total) and the chart drawing
   that year's twelve monthly bars.
2. **The first Nerd-stats chart should span the full width and is too tall.**
   The first chart in the Nerd-stats section (per-channel time series on the
   detail page, per-station on the summary page) sits inside a two-column grid,
   so it only takes half the width; its `aspect-[16/9]` height is too tall once
   it goes full width.
3. **The search dialog has two issues.** The results list is not scrollable, and
   the per-result action buttons ("Find on map" / "Open detail") exceed the
   dialog layout on narrow widths instead of being responsive.

## Scope

Frontend-only; no backend or API changes. The detail page
([`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:1))
and the summary page
([`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:1))
render the same components, so the chart fixes apply to both.

In scope: [`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:1),
the Nerd-stats layout in both pages, [`SearchDialog.tsx`](../frontend/src/features/search/SearchDialog.tsx:1),
[`StationListItem.tsx`](../frontend/src/features/stations/StationListItem.tsx:1),
and the `detail.spec.ts` e2e assertions.

Out of scope: the main "Detailed statistics" full-width chart (left unchanged),
the timeframe dropdown / compare checkbox behaviour, and any backend data shape.

## Decisions

- **Monthly bar chart** = the shadcn interactive bar-chart pattern, using the
  calendar **years** as the selectable series (the user supplied the shadcn
  example; its "Desktop"/"Mobile" series map to years here).
- **Nerd-stats first chart**: render it as a standalone full-width `ChartCard`
  above a `md:grid-cols-2` grid that holds the remaining two charts. Reduce its
  height by passing a shorter aspect-ratio class (suggest `aspect-[21/9]`;
  implementer may tune the exact value) to
  [`TimeSeriesLineChart`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:47).
- **Search dialog scroll**: replace the Radix `ScrollArea` with a plain
  `min-h-0 flex-1 overflow-y-auto` container. This is immune to the flexbox
  percentage-height pitfall (a `max-h` parent with an auto height and a
  `height: 100%` viewport) that currently prevents scrolling.
- **Responsive action buttons**: keep the icons, hide the text on small screens
  (`hidden sm:inline`) and add `aria-label` / `title` so the icon-only buttons
  stay accessible.

## Changes

### 1. Interactive monthly bar chart ([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:1))

Replace the single-series chronological chart with the interactive per-year
version:

- Derive the sorted unique years from `totals` (`[...new Set(...)].sort()`).
- Build `chartData` as 12 rows (Jan … Dec) where each row is
  `{ month: 'Jan', '2024': n, '2025': m, … }`; leave a year key absent when a
  month has no data so Recharts draws no bar for it.
- Build `chartConfig` keyed by year string, e.g.
  `{ '2024': { label: '2024', color: seriesColor(0) }, … }` reusing
  [`seriesColor`](../frontend/src/features/stationDetail/chartUtils.ts:14) so
  colours cycle the existing five chart tokens.
- Compute a per-year total for the header buttons.
- Hold the active year in `useState`, defaulting to the most recent year; when
  the active value is not among the current years (data reload), fall back to
  the latest year so state never goes stale.
- Header: left title/description; right one `<button>` per year (mirror the
  shadcn example styling, `data-active={...}` for the active state, with
  `flex-wrap` so many years stay responsive), each showing the year label and
  that year's formatted total.
- Body: `BarChart` with `XAxis dataKey="month"`, a single
  `<Bar dataKey={activeYear} fill={var(--color-${activeYear})} radius={4} />`,
  and a `ChartTooltip` whose label combines the month and active year
  (e.g. "Jan 2024").
- Empty `totals`: render the card with an empty state instead of an empty
  chart (reuse [`ChartEmptyState`](../frontend/src/features/stationDetail/ChartEmptyState.tsx:1)
  or a short muted message).

### 2. Full-width, shorter Nerd-stats chart — detail page ([`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:265))

In the Nerd-stats section, split the `grid md:grid-cols-2` into:

- a full-width `ChartCard` with the per-channel
  [`TimeSeriesLineChart`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:47)
  (title `cfg.perChannelTitle`), passing `className="aspect-[21/9]"`;
- a `grid grid-cols-1 gap-4 md:grid-cols-2` containing the "Weekdays by
  channel" radar and the "Share by channel" pie.

### 3. Full-width, shorter Nerd-stats chart — summary page ([`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:326))

Apply the same restructure to the per-station Nerd-stats: full-width
`TimeSeriesLineChart` (title `cfg.perChannelTitle`) with the shorter aspect
ratio, then the per-station weekday radar + share pie in a two-column grid.

### 4. Scrollable search results ([`SearchDialog.tsx`](../frontend/src/features/search/SearchDialog.tsx:69))

Replace the `ScrollArea` wrapper around the results `<ul>` with a plain
`<div className="min-h-0 flex-1 overflow-y-auto">`. Keep the dialog's
`max-h-[70vh]` + `overflow-hidden` and the pinned search-bar row. This makes the
list reliably scroll inside the capped dialog.

### 5. Responsive action buttons ([`StationListItem.tsx`](../frontend/src/features/stations/StationListItem.tsx:46))

For the "Find on map" and "Open detail" buttons:

- wrap the text in `<span className="hidden sm:inline">…</span>` so only the
  icon shows below `sm`;
- add `aria-label` and `title` to each button (e.g. "Find on map", "Open
  detail") so the icon-only state remains accessible.

### 6. e2e update ([`detail.spec.ts`](../frontend/e2e/detail.spec.ts:123))

The monthly-card assertion currently expects a `Total` text that no longer
exists. Replace it with an assertion that the "Bikes per month" card is visible
and contains at least one year button; optionally assert clicking a year button
switches the rendered bar series.

## Testing / gates

- `make frontend-build` (tsc + vite) green.
- `make test-playwright` green, including the updated `detail.spec.ts` and the
  unchanged `summary.spec.ts` / `search.spec.ts`.
- No backend files are touched, so `make check` / `make test` / `make test-rest`
  / `make coverage` are unaffected; re-run only if anything backend-adjacent is
  edited.

## Definition of done

- [x] Monthly chart groups by year with clickable year totals (interactive).
- [x] Nerd-stats first chart is full width with a reduced height on both pages.
- [x] Search results scroll inside the dialog; action buttons are responsive and
      accessible.
- [x] `detail.spec.ts` assertions updated; `make frontend-build` (tsc + vite) and
      `make test-playwright` (19 specs) green.
- [x] Plan registered in [`plans/README.md`](../plans/README.md:1).
