# 43 - Monthly bar chart: Y-axis labels, bikes unit and year-over-year trend

Status: completed

## Problem

The standalone "Bikes per month" bar chart on the detail page
([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:1))
has three usability gaps:

1. It renders only an `XAxis` — there is no `YAxis`, so the bars have no
   vertical tick labels and their magnitudes are unreadable.
2. The year buttons in the top-right header show each year's total as a bare
   number, so it is unclear that the value counts **bikes**.
3. There is no year-over-year comparison: the user wants a **percentage change
   (p-%)** with an **up/down arrow** relative to the previous year, shown on
   every year button.

## Scope

Frontend-only. No backend, API or data-model changes. Reuse the existing
[`TrendIcon`](../frontend/src/features/stationOverview/TrendIcon.tsx:1)
component and the `Trend` type from
[`stationOverview/types.ts`](../frontend/src/features/stationOverview/types.ts:1).

Out of scope: the timeframe dropdown, the other charts, the nerd stats, and any
change to the `monthly_totals` payload.

## Decisions / assumptions

- **Y axis**: mirror the existing line chart pattern
  ([`TimeSeriesLineChart.tsx`](../frontend/src/features/stationDetail/TimeSeriesLineChart.tsx:89)):
  `<YAxis orientation="left" tickLine={false} axisLine={false} width={40}
  allowDecimals={false} tickFormatter={...} />` with
  [`formatNumber`](../frontend/src/lib/format.ts:7) for readable ticks. The
  `orientation="left"` is explicit so the bar chart's Y axis sits on the **left**,
  the same side as every other chart's Y axis (the line charts already use the
  Recharts default left orientation).
- **Unit suffix**: append a lowercase `bikes` span after each year total in the
  top-right header, using the same muted style as
  [`MetricCard.tsx`](../frontend/src/features/stationOverview/MetricCard.tsx:24).
- **p-%**: computed client-side from the already-available `yearlyTotals` as
  `(total - previousTotal) / previousTotal * 100`, rounded to one decimal place
  and formatted with the shared `LOCALE` (`de-DE`). A year with no previous year
  (or a previous-year total of `0`) renders `–` with no arrow; otherwise the
  up/down/flat [`TrendIcon`](../frontend/src/features/stationOverview/TrendIcon.tsx:1)
  plus a colour-matched percentage.
- **Trend colouring**: up = `text-emerald-600`, down = `text-rose-600`,
  flat = `text-muted-foreground` (matches the arrow colours in `TrendIcon`).

## Frontend changes

### 1. Imports ([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:1))

- Add `YAxis` to the existing `recharts` import.
- Import `TrendIcon` from `../stationOverview/TrendIcon`.
- Import `type { Trend }` from `../stationOverview/types`.

### 2. Y-axis ([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:127))

Add the Y axis right after the existing `XAxis` inside the `BarChart`:

```tsx
<YAxis
  orientation="left"
  tickLine={false}
  axisLine={false}
  width={40}
  allowDecimals={false}
  tickFormatter={(value) => formatNumber(Number(value))}
/>
```

The existing `margin={{ left: 12, right: 12 }}` already leaves room; adjust only
if the ticks feel cramped in the browser.

### 3. Year-over-year totals ([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:78))

Replace the `yearlyTotals` memo with one that also computes the previous-year
comparison:

```ts
const yearlyTotals = useMemo(() => {
  const totalByYear = new Map<number, number>()
  for (const year of years) {
    totalByYear.set(
      year,
      totals.filter((entry) => entry.year === year).reduce((acc, entry) => acc + entry.total, 0),
    )
  }
  return years.map((year) => {
    const total = totalByYear.get(year) ?? 0
    const previous = totalByYear.get(year - 1)
    let deltaPercent: number | null = null
    let trend: Trend | null = null
    if (previous !== undefined && previous > 0) {
      deltaPercent = Math.round(((total - previous) / previous) * 1000) / 10
      trend = deltaPercent > 0 ? 'up' : deltaPercent < 0 ? 'down' : 'flat'
    }
    return { year, total, deltaPercent, trend }
  })
}, [totals, years])
```

### 4. Year button markup ([`MonthlyBarChart.tsx`](../frontend/src/features/stationDetail/MonthlyBarChart.tsx:102))

For each year button:

- Append a muted `bikes` suffix to the total.
- Add a third row below the total: the trend arrow (when `trend` is set) plus the
  percentage (or `–` when `deltaPercent` is `null`).

```tsx
<span className="text-lg leading-none font-bold sm:text-2xl">
  {formatNumber(total)}
  <span className="ml-1 text-xs font-normal text-muted-foreground">bikes</span>
</span>
<span className="flex items-center gap-1 text-xs font-semibold">
  {trend && <TrendIcon trend={trend} />}
  <span
    className={
      trend === 'up'
        ? 'text-emerald-600'
        : trend === 'down'
          ? 'text-rose-600'
          : 'text-muted-foreground'
    }
  >
    {deltaPercent === null ? '–' : `${deltaPercent > 0 ? '+' : ''}${formatNumber(deltaPercent)}%`}
  </span>
</span>
```

## Testing / gates

- Extend [`detail.spec.ts`](../frontend/e2e/detail.spec.ts:123) so the existing
  "monthly bar chart renders" test also asserts:
  - the monthly card has at least one `.recharts-yAxis .recharts-cartesian-axis-tick`
    (Y-axis labels are drawn), and
  - the first year button contains the `bikes` suffix.
  Keep assertions robust to the real Münster import (the oldest year has no
  previous year and therefore shows `–`).
- Run the frontend gates: `make frontend-build` (runs `tsc` + `vite build`) and
  `make test-playwright` (see [`agents.md`](../agents.md:25)). The backend gates
  are unaffected but `make check` is cheap to run for safety.

## Definition of done

- [x] `YAxis` with formatted tick labels added to the monthly bar chart
- [x] `bikes` suffix shown on every year button total
- [x] p-% + up/down/flat arrow shown per year (vs. the previous year), with `–`
      for the oldest year
- [x] e2e assertions updated
- [x] `make frontend-build` and `make test-playwright` green
