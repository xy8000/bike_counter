import { useMemo } from 'react'
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
import { formatNumber } from '../../lib/format'
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

const chartConfig = {
  total: { label: 'Bikes', color: 'var(--chart-1)' },
} satisfies ChartConfig

/// A bar chart with one bar per calendar month across all available years
/// (x-axis like "Jan 2024") and the grand total shown in the top-right corner.
/// Follows the shadcn interactive bar-chart pattern but is a single total series
/// and is **not** driven by the timeframe dropdown.
export function MonthlyBarChart({ totals }: { totals: MonthTotal[] }) {
  const data = useMemo(
    () =>
      totals.map((entry) => ({
        // Sort key that keeps calendar order (year * 100 + month).
        month: entry.year * 100 + entry.month,
        label: `${MONTH_NAMES[entry.month - 1] ?? ''} ${entry.year}`.trim(),
        total: entry.total,
      })),
    [totals],
  )
  const grandTotal = useMemo(
    () => totals.reduce((acc, entry) => acc + entry.total, 0),
    [totals],
  )

  return (
    <Card className="py-0">
      <CardHeader className="flex flex-col items-stretch border-b p-0 sm:flex-row">
        <div className="flex flex-1 flex-col justify-center gap-1 px-6 pt-4 pb-3 sm:py-4">
          <CardTitle>Bikes per month</CardTitle>
          <CardDescription>All available months, grouped by year</CardDescription>
        </div>
        <div className="flex items-center justify-end border-t px-6 py-4 sm:border-t-0 sm:border-l sm:px-8">
          <div className="text-right">
            <span className="text-xs text-muted-foreground">Total</span>
            <span className="block text-lg leading-none font-bold sm:text-3xl">
              {formatNumber(grandTotal)}
            </span>
          </div>
        </div>
      </CardHeader>
      <CardContent className="px-2 sm:p-6">
        <ChartContainer config={chartConfig} className="aspect-auto h-[250px] w-full">
          <BarChart accessibilityLayer data={data} margin={{ left: 12, right: 12 }}>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="label"
              tickLine={false}
              axisLine={false}
              tickMargin={8}
              minTickGap={32}
            />
            <ChartTooltip
              cursor={false}
              content={<ChartTooltipContent className="w-[140px]" labelKey="label" />}
            />
            <Bar dataKey="total" fill="var(--color-total)" radius={4} />
          </BarChart>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}

