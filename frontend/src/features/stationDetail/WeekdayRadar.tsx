import { PolarAngleAxis, PolarGrid, Radar, RadarChart } from 'recharts'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import type { WeekdayTotal } from './types'
import { ChartEmptyState } from './ChartEmptyState'
import { ChartLimitNotice } from './ChartLimitNotice'
import { MAX_DATA_STREAMS, seriesColor } from './chartUtils'

export interface RadarSeries {
  key: string
  label: string
  data: WeekdayTotal[]
}

const WEEKDAY_LABELS = [
  'Monday',
  'Tuesday',
  'Wednesday',
  'Thursday',
  'Friday',
  'Saturday',
  'Sunday',
]

/// Radar over the seven weekdays (ISO 1 = Monday). One radar per series: for the
/// aggregate that is a single "Bikes" series, for the detailed stats one per
/// channel.
/// Missing weekdays are filled with 0 so the circle is always complete — this is
/// a fixed 7-slot axis, not zero-filled time buckets.
export function WeekdayRadar({ series, className }: { series: RadarSeries[]; className?: string }) {
  // A per-channel/per-station radar with more than MAX_DATA_STREAMS series is
  // unreadable; show an info note instead of rendering it.
  if (series.length > MAX_DATA_STREAMS) {
    return <ChartLimitNotice className={cn('aspect-square', className)} />
  }

  // Recharts' RadarChart crashes when a radar's data is empty (or all zero) —
  // the current window simply has no traffic yet — so fall back to the same
  // empty state the other charts use instead of feeding it empty rows.
  const hasData = series.some((item) => item.data.some((day) => day.total > 0))
  if (!hasData) {
    return (
      <ChartEmptyState
        message="No traffic for this period."
        className={cn('aspect-square', className)}
      />
    )
  }

  const rows = WEEKDAY_LABELS.map((label, i) => {
    const row: Record<string, string | number> = { weekday: label }
    for (const { key, data } of series) {
      row[key] = data.find((day) => day.weekday === i + 1)?.total ?? 0
    }
    return row
  })
  const config: ChartConfig = Object.fromEntries(
    series.map((item, index) => [item.key, { label: item.label, color: seriesColor(index) }]),
  )

  return (
    <ChartContainer
      config={config}
      className={cn('mx-auto aspect-square max-w-[320px]', className)}
    >
      <RadarChart data={rows} outerRadius="70%">
        <ChartTooltip cursor={false} content={<ChartTooltipContent />} />
        <PolarAngleAxis dataKey="weekday" tickLine={false} tick={{ fontSize: 11 }} />
        <PolarGrid />
        {series.map(({ key }) => (
          <Radar
            key={key}
            dataKey={key}
            fill={`var(--color-${key})`}
            fillOpacity={0.3}
            stroke={`var(--color-${key})`}
            strokeWidth={2}
            isAnimationActive={false}
          />
        ))}
      </RadarChart>
    </ChartContainer>
  )
}
