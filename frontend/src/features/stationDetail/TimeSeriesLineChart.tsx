import { CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts'
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import type { TimeBucket } from './types'
import { ChartEmptyState } from './ChartEmptyState'
import { seriesColor } from './chartUtils'

export interface LineSeries {
  key: string
  label: string
  data: TimeBucket[]
}

type MergedPoint = { time: number; [series: string]: number | null }

/// Merge the series' buckets (keyed by their bucket start) into one point per
/// timestamp so Recharts can draw several lines on a shared time axis. Points
/// exist only where there is data — no zero-filling.
function mergeSeries(series: LineSeries[]): MergedPoint[] {
  const byTime = new Map<number, MergedPoint>()
  for (const { key, data } of series) {
    for (const bucket of data) {
      const time = new Date(bucket.start).getTime()
      const point = byTime.get(time) ?? { time }
      point[key] = bucket.total
      byTime.set(time, point)
    }
  }
  return [...byTime.values()].sort((a, b) => a.time - b.time)
}

/// A line chart for one or more time-series built on the shadcn `chart`
/// component. The optional `xDomain` lets the caller extend the axis past the
/// latest data point so a running window (e.g. the current week) shows its
/// leftover days without fabricating buckets.
///
/// Series without any data are dropped before rendering, and the legend is only
/// drawn when more than one series actually has data — so the chart never shows
/// legend entries for channels that have no traffic.
export function TimeSeriesLineChart({
  series,
  xFormatter,
  tooltipFormatter,
  xDomain,
  className,
}: {
  series: LineSeries[]
  xFormatter: (time: number) => string
  tooltipFormatter?: (time: number) => string
  xDomain?: [number, number]
  className?: string
}) {
  const visibleSeries = series.filter((item) => item.data.length > 0)
  const data = mergeSeries(visibleSeries)
  const config: ChartConfig = Object.fromEntries(
    visibleSeries.map((item, index) => [
      item.key,
      { label: item.label, color: seriesColor(index) },
    ]),
  )
  const labelFor = tooltipFormatter ?? xFormatter

  if (visibleSeries.length === 0) {
    return <ChartEmptyState className={cn('aspect-[20/9]', className)} />
  }

  return (
    <ChartContainer config={config} className={cn('aspect-[20/9]', className)}>
      <LineChart data={data} margin={{ top: 8, right: 12, bottom: 0, left: 0 }}>
        <CartesianGrid vertical={false} />
        <XAxis
          dataKey="time"
          type="number"
          scale="time"
          domain={xDomain ?? ['dataMin', 'dataMax']}
          tickLine={false}
          axisLine={false}
          tickMargin={8}
          tickFormatter={xFormatter}
          minTickGap={24}
        />
        <YAxis tickLine={false} axisLine={false} width={40} allowDecimals={false} />
        <ChartTooltip
          cursor={false}
          content={
            <ChartTooltipContent
              labelFormatter={(_, payload) => {
                const first = payload[0]
                const time =
                  typeof first?.payload?.time === 'number' ? first.payload.time : NaN
                return labelFor(Number.isFinite(time) ? time : Date.now())
              }}
            />
          }
        />
        {visibleSeries.length > 1 && <ChartLegend content={<ChartLegendContent />} />}
        {visibleSeries.map((item) => (
          <Line
            key={item.key}
            dataKey={item.key}
            type="monotone"
            stroke={`var(--color-${item.key})`}
            strokeWidth={2}
            dot={false}
            isAnimationActive={false}
          />
        ))}
      </LineChart>
    </ChartContainer>
  )
}
