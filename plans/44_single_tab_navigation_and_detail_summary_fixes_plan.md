# 44 - Single-tab navigation, responsive search actions and detail/summary fixes

Status: in progress

## Problem

Four related frontend defects on the map / search / detail / summary views:

1. Several station-detail links open a **new browser tab** instead of staying in
   the SPA: the station-name heading and the "Open detail page" icon in the
   overview panel ([`StationOverview.tsx`](../frontend/src/features/stationOverview/StationOverview.tsx:27)),
   and the same two links in the map marker popup
   ([`MapView.tsx`](../frontend/src/features/map/MapView.tsx:99)). The app should
   stay in one tab.
2. In the **search results**, the "Find on map" and "Open detail" action buttons
   ([`StationListItem.tsx`](../frontend/src/features/stations/StationListItem.tsx:46))
   exceed the layout on narrower widths.
3. The detail-page timeframe dropdown labels claim a comparison that is not shown
   unless the "Compare previous period" checkbox is ticked: "Current + last week"
   and "Last year"
   ([`timeframes.ts`](../frontend/src/features/stationDetail/timeframes.ts:75)).
4. The first (main) time-series chart is too tall on the detail and summary pages;
   both pages already share [`TimeSeriesLineChart`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:47)
   and should keep doing so.

## Scope

Frontend-only. No backend, API or data-model changes. The Playwright e2e specs
must be updated to match the new labels and the single-tab navigation.

## Decisions / assumptions

- **Same-tab navigation**: replace `target="_blank" rel="noreferrer"` anchors with
  React Router `<Link>` (SPA navigation, no full reload). The link role + `href`
  are preserved, so the existing `getByRole('link')` locators keep working.
- **Responsive search actions**: the list row wraps so the two action buttons move
  below the station text on narrow viewports instead of pushing past the edge; the
  station name also truncates so long names cannot force horizontal overflow. The
  buttons keep their `aria-label`/`title`, so the accessible names ("Find on map",
  "Open detail") stay stable for the e2e locators.
- **Timeframe naming**: dropdown labels describe only the base period (comparison
  is opt-in via the checkbox and shown through the legend):
  - `week`: `label`/`title`/`perChannelTitle` → `This week` / `This week` /
    `This week by channel`; `subtitle` → `1-hour buckets`.
  - `year`: `label`/`title`/`perChannelTitle` → `This year` / `This year` /
    `This year by channel`; `subtitle` → `1-day buckets`.
  - `day` (`24 hours`) and `last_30_days` (`Last 30 days`) are unchanged.
  - `currentLabel`/`previousLabel` (the chart legend) are unchanged, so the
    "Current week" / "Last week" and "Current year" / "Last year" legend entries
    remain.
- **Chart height**: the first graph uses `TimeSeriesLineChart`'s default
  `aspect-[16/9]`. A 20% shorter height is `aspect-[20/9]` (height =
  width × 9/20 instead of width × 9/16). Change the component default (both the
  empty state and the chart container) so the detail and summary pages — which
  share this component — change together. The "Nerd stats" charts pass an
  explicit `aspect-[21/9]`, which keeps overriding the default, so they are
  unaffected.

## Frontend changes

### 1. Overview panel ([`StationOverview.tsx`](../frontend/src/features/stationOverview/StationOverview.tsx:27))

- Import `Link` from `react-router-dom`.
- Replace the name heading `<a href={overview.detail_url} target="_blank"
  rel="noreferrer">` with `<Link to={`/stations/${stationId}`}>` keeping the same
  className (still a link, heading-styled, not new-tab).
- Replace the icon `<Button asChild><a href={overview.detail_url} target="_blank"
  rel="noreferrer">` with `<Button asChild><Link to={`/stations/${stationId}`}>`
  (keeps the `Open detail page` accessible name).

### 2. Map popup ([`MapView.tsx`](../frontend/src/features/map/MapView.tsx:95))

- Import `Link` from `react-router-dom`.
- Replace the two popup `<a href={`/stations/${station.id}`} target="_blank"
  rel="noreferrer">` links (name + `Open detail page` icon) with
  `<Link to={`/stations/${station.id}`}>`, keeping the same classes and labels.

### 3. Search actions ([`StationListItem.tsx`](../frontend/src/features/stations/StationListItem.tsx:25))

- Let the `<li>` wrap (`flex-wrap`) so the action buttons drop below the station
  text on narrow widths while staying in a row on wider screens.
- Give the main station button a full-width basis on small screens and restore
  `flex-1` on `sm:` and up.
- Add `truncate` to the station-name span so long names cannot force horizontal
  overflow.
- Keep both action buttons `shrink-0` with their `aria-label`/`title` intact.

### 4. Timeframe naming ([`timeframes.ts`](../frontend/src/features/stationDetail/timeframes.ts:75))

- `week`: `label: 'This week'`, `title: 'This week'`, `subtitle: '1-hour buckets'`,
  `perChannelTitle: 'This week by channel'`.
- `year`: `label: 'This year'`, `title: 'This year'`, `subtitle: '1-day buckets'`,
  `perChannelTitle: 'This year by channel'`.

### 5. Detail comment ([`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:142))

- Update the default-timeframe comment from `"Current + last week"` to
  `"This week"`.

### 6. First-graph height ([`TimeSeriesLineChart.tsx`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:71))

- Change the default `aspect-[16/9]` to `aspect-[20/9]` in both the
  `ChartEmptyState` fallback and the `ChartContainer` (20% shorter). This is the
  shared component used by the first graph on both the detail and summary pages.

## e2e changes

### [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:133)

- Default-timeframe assertion: `'1-hour buckets — the weeks are overlapped'` →
  `'1-hour buckets'`, and update the comment from `"Current + last week"` to
  `"This week"`. The "Last 30 days" option assertion stays.

### [`map.spec.ts`](../frontend/e2e/map.spec.ts:4)

- Extend the popup test to click the popup's "Open detail page" link and assert
  the URL becomes `/stations/<id>` with only one page in the context (no new tab).
- Extend the overview-panel test to click the overview's "Open detail page" link
  and assert the same single-tab navigation. The existing `href` assertions stay.

## Gates

- `make test-playwright` (frontend UI changed).
- Backend untouched, so `make check` / `make test` / `make coverage` are expected
  to stay green and are re-run to be safe.
- Update [`plans/README.md`](../plans/README.md:1) registration.
