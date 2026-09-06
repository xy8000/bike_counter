# 115 - Fix X-axis labels on the settings-driven bar charts

Status: implemented (make check + make test-playwright green)

## Problem

The Bike-Trends bar charts render unreadable x-axes for two settings-driven
combinations:

- **Last 30 days + 1-hour buckets (compare on):** every visible tick reads
  `00:00`. The 720 hourly buckets are labelled with a time-only formatter
  ([`timeAxis('hour')`](../frontend/src/features/stationDetail/timeframes.ts:14) →
  `HH:mm`), and recharts thins the category axis to a multiple of 24 buckets, so
  every shown tick lands on midnight of a different day and the label repeats.
- **This year + 1-week buckets (compare on):** the labels collapse to
  `JanJanFebFebMärMär…`. The year timeframe labels every bucket with a bare
  month short name ([`timeAxis('month')`](../frontend/src/features/stationDetail/timeframes.ts:22)),
  so 4–5 consecutive weekly buckets (or ~30 daily buckets) map to the same
  `Jan`/`Feb`/… label and the repeated text overlaps. The Y axis shows the same
  symptom for German-formatted numbers (`0`, `300.000`, … `1.200.000`): the
  fixed 68 px width is too narrow and the tick labels overlap.

Root cause: the x-axis formatter is chosen from *timeframe + resolution* only
([`fixedAxis`](../frontend/src/features/stationDetail/timeframes.ts:265),
[`customTimeframeConfig`](../frontend/src/features/stationDetail/timeframes.ts:304))
without making labels **unique per bucket** and without controlling **tick
density**. [`TimeSeriesBarChart`](../frontend/src/features/stationDetail/TimeSeriesBarChart.tsx:88)
always uses `minTickGap={24}` and never caps/angles the ticks.

## Goal / Decisions

1. Make the x-axis formatter emit **non-repeating, context-appropriate** labels
   for every `timeframe × resolution` combination the settings can produce.
2. Keep Recharts' width-aware tick spacing (it measures each rendered label and
   drops any that would overlap) and rotate long labels on dense axes so they
   stay readable on desktop and mobile.
3. Widen the Y axis so German `1.234.567`-style tick labels fit.
4. Cover both reported cases with Playwright e2e assertions.
5. Scope is the **settings-driven** time-series bar charts only (detail +
   summary pages). [`MonthlyBarChart`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:38)
   is explicitly not driven by the settings and is out of scope.

## Settings-driven combinations (current → target axis label)

| Timeframe | Resolution | Current axis | Issue | Target |
|---|---|---|---|---|
| 24 hours | 15m / 30m / hour | `HH:mm` | none (single day) | keep |
| This week | 30m / hour | `Mo HH:mm` | none | keep |
| This week | day | `Mo` | none | keep |
| Last 30 days | hour | `HH:mm` | repeats `00:00` | `dd.MM., HH:mm` |
| Last 30 days | day / week | `dd.MM.` | fine | keep |
| This year | day / week | `MMM` | repeats `JanJanFebFeb` | dedupe → one month label |
| This year | month | `MMM` | fine | keep |
| Individual ≤ 2 days | 15m / 30m / hour | `HH:mm` | fine | keep |
| Individual > 2 days | 15m / 30m / hour | `HH:mm` | repeats daily | `dd.MM., HH:mm` |
| Individual | day | `dd.MM.` | dense / ambiguous across year | `dd.MM.yy` when crossing years |
| Individual | week | `dd.MM.` | dense / ambiguous across years | `dd.MM.yy` when crossing years |
| Individual | month | `MMM yyyy` | fine | keep |
| Individual | quarter | `Q<n> yyyy` | fine | keep |

## Approach

### Task 1 — Context-correct x-axis formatters in `timeframes.ts`

Add three small helpers next to [`timeAxis`](../frontend/src/features/stationDetail/timeframes.ts:14):

- `dayTimeAxis(time)` → `dd.MM., HH:mm` (day + time), for sub-day buckets that
  span more than one day.
- `dayYearAxis(time)` → `dd.MM.yy` (compact, unambiguous across a year
  boundary), for long day/week-granularity individual ranges.
- `dedupe(axis)` → wraps a formatter and returns `''` when the current label
  equals the previous one, collapsing consecutive duplicates (e.g. repeated
  month names) to a single label. A fresh closure is created per config, so the
  per-render tick sequence resets cleanly.

Rewrite [`fixedAxis`](../frontend/src/features/stationDetail/timeframes.ts:265):

- `last_30_days` + `hour` → `dayTimeAxis` (fixes the `00:00` repetition).
- `year` + `day` / `week` → `dedupe(timeAxis('month'))` (fixes `JanJanFebFeb`).
- `year` + `month` → `timeAxis('month')` unchanged.
- `day` / `week` / `last_30_days` day+week branches unchanged.

### Task 2 — Span-aware custom-range axis

Change
[`customTimeframeConfig(granularity)`](../frontend/src/features/stationDetail/timeframes.ts:304)
to `customTimeframeConfig(granularity, from, to)` and derive the label density
from the range (compute the inclusive day count with the existing
[`dateFromInput`](../frontend/src/features/stationDetail/timeframes.ts:200), no
need to export [`rangeDays`](../frontend/src/features/stationDetail/resolution.ts:62)):

- sub-day (`15m` / `30m` / `hour`): span ≤ 2 days → `timeAxis('hour')`, else
  `dayTimeAxis`.
- `day` / `week`: if `from` and `to` fall into different calendar years (or the
  span is a multi-year range) → `dayYearAxis`, else `timeAxis('day')`.
- `month` / `quarter`: unchanged (`monthAxis` / `quarterAxis`).

Update the two call sites to pass `from, to`:
[`StationDetail.tsx`](../frontend/src/features/stationDetail/StationDetail.tsx:268)
and [`StationsSummary.tsx`](../frontend/src/features/stationsSummary/StationsSummary.tsx:330).

### Task 3 — Width-aware spacing + angled labels in `TimeSeriesBarChart`

In [`TimeSeriesBarChart`](../frontend/src/features/stationDetail/TimeSeriesBarChart.tsx:64):

- Set the [`XAxis`](../frontend/src/features/stationDetail/TimeSeriesBarChart.tsx:88)
  `interval="preserveStartEnd"` and keep `minTickGap`. Recharts 3's category
  spacing then **measures each label's real width and drops ticks that would
  overlap**. A numeric `interval` was tried first but force-renders every nth
  tick regardless of width (the week/30-minute view showed 24 crammed
  `Mo 00:00` labels) — do not use one. `preserveStartEnd` also keeps the first
  tick, so the first-of-run month label survives the duplicate collapse.
- Long labels rotate -45° (`angle`, `textAnchor="end"`, extra `height: 64` so a
  rotated axis reserves more vertical room), driven by the config's
  `axisRotate` hint. Set for `last_30_days` hour, week 30m/hour, and the
  individual sub-day multi-day views.
- Widen the [`YAxis`](../frontend/src/features/stationDetail/TimeSeriesBarChart.tsx:97)
  `width` from `68` to `80` so German `1.200.000`-style labels do not clip into
  the plot (keep the `formatNumber` formatter unchanged).

`tickFormatter` / `tooltip` / legend behaviour stay as-is — only the new
`interval`, `minTickGap`, `angle`, `textAnchor`, `height` and `width` values
change.

### Task 4 — Playwright e2e coverage

Extend [`settings.spec.ts`](../frontend/e2e/settings.spec.ts:1) (or
[`detail.spec.ts`](../frontend/e2e/detail.spec.ts:248)) with two assertions,
using the recharts tick-label locator pattern already used for the monthly
chart (`… .recharts-cartesian-axis-tick-value`):

- **Last 30 days + Hour:** select `Last 30 days` and the `Hour` resolution,
  then assert the x-axis tick labels contain more than one distinct value and
  at least one label matches a `dd.MM., HH:mm` shape (i.e. not all `00:00`).
- **This year + Week:** select `This year` and the `Week` resolution, then
  assert the x-axis tick texts contain no consecutive duplicate label.

### Task 5 — Gates

- `make check` (prettier + `tsc` build; backend untouched).
- `make test-playwright` (frontend UI change).

## Follow-up (during review)

- **This week + 30-minute buckets:** a first version forced a numeric tick
  `interval`, which made recharts render every nth tick without width
  measurement — 24 wide `Mo 00:00` labels overlapped. Resolved by reverting to
  the width-aware `preserveStartEnd` spacing and rotating the long week
  sub-day labels (`fixedAxisRotate` now also covers week 30m/hour).
- **Rotated labels sit ~25% tighter:** rotated axes use `minTickGap={18}` (vs
  `24` horizontal), because a -45° label's angled footprint is smaller, so more
  labels fit; the extra `height: 64` reserves the room the rotated axis needs.

## Notes

- The exact `MAX_X_TICKS` value and `height` can be tuned during
  implementation; the invariant is: distinct labels per tick and no overlap at
  phone/tablet viewports (the responsive e2e runs those sizes).
- [`MonthlyBarChart`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:38)
  uses a fixed 12-slot month axis and is intentionally not driven by the
  settings; it is left unchanged.
- No backend or REST changes are expected, so `make test-rest` / full
  `make test` are only needed if something regresses.
