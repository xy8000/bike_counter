import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from 'recharts'
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import { formatNumber } from '../../lib/format'
import type { TimeBucket } from './types'
import { ChartEmptyState } from './ChartEmptyState'
import { ChartLimitNotice } from './ChartLimitNotice'
import { MAX_DATA_STREAMS, seriesColor } from './chartUtils'

export interface BarSeries {
  key: string
  label: string
  /** Bars with the same `stackId` stack together (e.g. all channels of the
   *  current period); different `stackId`s are drawn side-by-side (e.g. the
   *  current period next to the previous period). */
  stackId: string
  data: TimeBucket[]
}

type MergedPoint = { time: number; [series: string]: number | null }

/// Merge the series' buckets (keyed by their bucket start) into one point per
/// timestamp so Recharts can draw several bars on a shared time axis. Points
/// exist only where there is data — no zero-filling.
function mergeSeries(series: BarSeries[]): MergedPoint[] {
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

/// A bar chart for one or more time-series built on the shadcn `chart`
/// component. Series sharing a `stackId` (e.g. several channels of the current
/// period) stack into one bar, while series with different `stackId`s (e.g.
/// the previous period) render as separate side-by-side bars.
///
/// Series without any data are dropped before rendering, and the legend is only
/// drawn when more than one series actually has data — so the chart never shows
/// legend entries for channels that have no traffic.
export function TimeSeriesBarChart({
  series,
  xFormatter,
  axisRotate = false,
  tooltipFormatter,
  className,
}: {
  series: BarSeries[]
  xFormatter: (time: number) => string
  /** True when the view's x-axis labels are long (e.g. `dd.MM., HH:mm` across 30
   *  days), so they are rotated -45° and given more room on a dense axis. */
  axisRotate?: boolean
  tooltipFormatter?: (time: number) => string
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

  // A per-channel/per-station chart with more than MAX_DATA_STREAMS series is
  // unreadable; show an info note instead of rendering it.
  if (visibleSeries.length > MAX_DATA_STREAMS) {
    return <ChartLimitNotice className={cn('aspect-[20/9]', className)} />
  }

  return (
    <ChartContainer config={config} className={cn('aspect-[20/9]', className)}>
      <BarChart accessibilityLayer data={data} margin={{ top: 8, right: 12, bottom: 0, left: 0 }}>
        <CartesianGrid vertical={false} />
        {/* `interval="preserveStartEnd"` keeps Recharts' width-aware tick
            spacing (it measures each label and drops any that would overlap —
            a numeric `interval` would force-render every nth tick and overlap on
            narrow widths). Preserving the start also keeps the first-of-run
            month labels from being skipped by the duplicate collapse. Rotated
            axes reserve extra height for the -45° labels and can sit ~25%
            tighter (their angled footprint is smaller). */}
        <XAxis
          dataKey="time"
          type="category"
          tickLine={false}
          axisLine={false}
          tickMargin={axisRotate ? 12 : 8}
          tickFormatter={xFormatter}
          minTickGap={axisRotate ? 18 : 24}
          interval="preserveStartEnd"
          angle={axisRotate ? -45 : 0}
          textAnchor={axisRotate ? 'end' : 'middle'}
          height={axisRotate ? 64 : 30}
        />
        <YAxis
          tickLine={false}
          axisLine={false}
          width={80}
          tickMargin={8}
          allowDecimals={false}
          tickFormatter={(value) => formatNumber(Number(value))}
        />
        <ChartTooltip
          cursor={false}
          content={
            <ChartTooltipContent
              labelFormatter={(_, payload) => {
                const first = payload[0]
                const time = typeof first?.payload?.time === 'number' ? first.payload.time : NaN
                return labelFor(Number.isFinite(time) ? time : Date.now())
              }}
            />
          }
        />
        {visibleSeries.length > 1 && <ChartLegend content={<ChartLegendContent />} />}
        {visibleSeries.map((item) => (
          <Bar
            key={item.key}
            dataKey={item.key}
            stackId={item.stackId}
            fill={`var(--color-${item.key})`}
            radius={2}
            isAnimationActive={false}
          />
        ))}
      </BarChart>
    </ChartContainer>
  )
}
