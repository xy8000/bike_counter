import { useMemo, useState } from 'react'
import { Bar, BarChart, CartesianGrid, XAxis } from 'recharts'

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import { formatNumber } from '../../lib/format'
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
  const years = useMemo(
    () => [...new Set(totals.map((entry) => entry.year))].sort(),
    [totals],
  )

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

  const yearlyTotals = useMemo(
    () =>
      years.map((year) => ({
        year,
        total: totals
          .filter((entry) => entry.year === year)
          .reduce((acc, entry) => acc + entry.total, 0),
      })),
    [totals, years],
  )

  const [activeYear, setActiveYear] = useState<string | null>(null)
  // Fall back to the most recent year when none is selected or the selected one
  // no longer exists (e.g. the station's data was reloaded).
  const currentYear = activeYear ?? String(years[years.length - 1])

  return (
    <Card className="py-0">
      <CardHeader className="flex flex-col items-stretch border-b p-0 sm:flex-row">
        <div className="flex flex-1 flex-col justify-center gap-1 px-6 pt-4 pb-3 sm:py-4">
          <CardTitle>Bikes per month</CardTitle>
          <CardDescription>All available months, grouped by year</CardDescription>
        </div>
        <div className="flex flex-wrap border-t sm:border-t-0 sm:border-l">
          {yearlyTotals.map(({ year, total }) => (
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
              </span>
            </button>
          ))}
        </div>
      </CardHeader>
      <CardContent className="px-2 sm:p-6">
        {years.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            No monthly data yet.
          </p>
        ) : (
          <ChartContainer config={chartConfig} className="aspect-auto h-[250px] w-full">
            <BarChart accessibilityLayer data={data} margin={{ left: 12, right: 12 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="month"
                tickLine={false}
                axisLine={false}
                tickMargin={8}
                minTickGap={16}
              />
              <ChartTooltip
                cursor={false}
                content={
                  <ChartTooltipContent
                    className="w-[140px]"
                    labelFormatter={(_, payload) => {
                      const first = payload[0]
                      const month =
                        typeof first?.payload?.month === 'string'
                          ? first.payload.month
                          : ''
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
