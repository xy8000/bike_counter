import { useMemo, useState } from 'react'
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader } from '@/components/ui/card'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import { formatNumber } from '../../lib/format'
import { TrendIcon } from '../stationOverview/TrendIcon'
import type { Trend } from '../stationOverview/types'
import { seriesColor } from './chartUtils'
import type { MonthTotal } from './types'

const MONTH_NAMES = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
]

/// The shadcn interactive bar chart with one series per calendar year: the
/// header lists every available year as a clickable button (showing that year's
/// total) and the chart draws the selected year's twelve monthly bars. Follows
/// the interactive bar-chart pattern and is **not** driven by the timeframe
/// dropdown.
export function MonthlyBarChart({ totals }: { totals: MonthTotal[] }) {
  // Sorted distinct years that have data.
  const years = useMemo(() => [...new Set(totals.map((entry) => entry.year))].sort(), [totals])

  // One row per month with a column per year; a missing year key means no data
  // for that month, so Recharts draws no bar for it.
  const data = useMemo(
    () =>
      MONTH_NAMES.map((monthName, index) => {
        const month = index + 1
        const row: Record<string, string | number> = { month: monthName }
        for (const year of years) {
          const entry = totals.find((total) => total.year === year && total.month === month)
          if (entry) row[String(year)] = entry.total
        }
        return row
      }),
    [totals, years],
  )

  // Per-year colour + label config, keyed by the year string (ChartStyle emits
  // `--color-<year>` from these so `fill="var(--color-<year>)"` resolves).
  const chartConfig = useMemo(
    () =>
      Object.fromEntries(
        years.map((year, index) => [
          String(year),
          { label: String(year), color: seriesColor(index) },
        ]),
      ) satisfies ChartConfig,
    [years],
  )

  // Per-year total plus the year-over-year comparison vs the previous year: the
  // percentage change (p-%, one decimal) and the up/down/flat trend. A year
  // without a previous year (or a previous year that totals 0) has no p-%, and
  // neither does the most recent year — it is always incomplete (still being
  // imported), so a trend vs the previous full year would be misleading.
  const yearlyTotals = useMemo(() => {
    const totalByYear = new Map<number, number>()
    for (const year of years) {
      totalByYear.set(
        year,
        totals.filter((entry) => entry.year === year).reduce((acc, entry) => acc + entry.total, 0),
      )
    }
    const latestYear = years[years.length - 1]
    return years.map((year) => {
      const total = totalByYear.get(year) ?? 0
      const previous = totalByYear.get(year - 1)
      let deltaPercent: number | null = null
      let trend: Trend | null = null
      if (year !== latestYear && previous !== undefined && previous > 0) {
        deltaPercent = Math.round(((total - previous) / previous) * 1000) / 10
        trend = deltaPercent > 0 ? 'up' : deltaPercent < 0 ? 'down' : 'flat'
      }
      return { year, total, deltaPercent, trend }
    })
  }, [totals, years])

  const [activeYear, setActiveYear] = useState<string | null>(null)
  // Fall back to the most recent year when none is selected or the selected one
  // no longer exists (e.g. the station's data was reloaded).
  const currentYear = activeYear ?? String(years[years.length - 1])

  return (
    <Card className="py-0">
      {/* The year selector (one button per year) is the card's header; the
          section above provides the "Bikes per month" heading + remark, so the
          card itself carries no duplicate title/description. */}
      <CardHeader className="flex flex-wrap items-stretch border-b p-0">
        {yearlyTotals.map(({ year, total, deltaPercent, trend }) => (
          <button
            key={year}
            type="button"
            data-active={currentYear === String(year)}
            className={cn(
              'flex flex-1 flex-col justify-center gap-1 px-4 py-3 text-left data-[active=true]:bg-muted/50 sm:px-6 sm:py-4',
            )}
            onClick={() => setActiveYear(String(year))}
          >
            <span className="text-xs text-muted-foreground">{year}</span>
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
                {deltaPercent === null
                  ? '–'
                  : `${deltaPercent > 0 ? '+' : ''}${formatNumber(deltaPercent)}%`}
              </span>
            </span>
          </button>
        ))}
      </CardHeader>
      <CardContent className="px-2 sm:p-6">
        {years.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">No monthly data yet.</p>
        ) : (
          <ChartContainer config={chartConfig} className="aspect-auto h-[250px] w-full">
            <BarChart accessibilityLayer data={data} margin={{ left: 12, right: 12 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="month"
                tickLine={false}
                axisLine={false}
                tickMargin={8}
                height={32}
                minTickGap={16}
              />
              <YAxis
                orientation="left"
                tickLine={false}
                axisLine={false}
                width={68}
                tickMargin={8}
                allowDecimals={false}
                tickFormatter={(value) => formatNumber(Number(value))}
              />
              <ChartTooltip
                cursor={false}
                content={
                  <ChartTooltipContent
                    className="w-[140px]"
                    labelFormatter={(_, payload) => {
                      const first = payload[0]
                      const month =
                        typeof first?.payload?.month === 'string' ? first.payload.month : ''
                      return `${month} ${currentYear}`.trim()
                    }}
                  />
                }
              />
              <Bar dataKey={currentYear} fill={`var(--color-${currentYear})`} radius={4} />
            </BarChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  )
}
