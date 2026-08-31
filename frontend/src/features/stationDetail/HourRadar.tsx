import { PolarAngleAxis, PolarGrid, Radar, RadarChart } from 'recharts'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import type { HourTotal } from './types'
import { ChartEmptyState } from './ChartEmptyState'
import { ChartLimitNotice } from './ChartLimitNotice'
import { MAX_DATA_STREAMS, seriesColor } from './chartUtils'

export interface HourRadarSeries {
  key: string
  label: string
  data: HourTotal[]
}

/// Hour-of-day axis labels `00` .. `23` (local time).
const HOUR_LABELS = Array.from({ length: 24 }, (_, hour) => String(hour).padStart(2, '0'))

/// Radar over the 24 local hours (0 = midnight .. 23 = 23:00). One radar per
/// series: for the aggregate that is a single "Bikes" series, for the detailed
/// stats one per channel/station. Missing hours are filled with 0 so the circle
/// is
/// always complete — a fixed 24-slot axis, not zero-filled time buckets.
export function HourRadar({
  series,
  className,
}: {
  series: HourRadarSeries[]
  className?: string
}) {
  // A per-channel/per-station radar with more than MAX_DATA_STREAMS series is
  // unreadable; show an info note instead of rendering it.
  if (series.length > MAX_DATA_STREAMS) {
    return <ChartLimitNotice className={cn('aspect-square', className)} />
  }

  // Mirror the WeekdayRadar guard: Recharts' RadarChart crashes on empty/all-zero
  // data (e.g. a window with no traffic yet), so show the shared empty state.
  const hasData = series.some((item) => item.data.some((hour) => hour.total > 0))
  if (!hasData) {
    return (
      <ChartEmptyState
        message="No traffic for this period."
        className={cn('aspect-square', className)}
      />
    )
  }

  const rows = HOUR_LABELS.map((label, hour) => {
    const row: Record<string, string | number> = { hour: label }
    for (const { key, data } of series) {
      row[key] = data.find((entry) => entry.hour === hour)?.total ?? 0
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
        <PolarAngleAxis dataKey="hour" tickLine={false} tick={{ fontSize: 10 }} />
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
