import { PolarAngleAxis, PolarGrid, Radar, RadarChart } from 'recharts'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import type { WeekdayTotal } from './types'
import { seriesColor } from './chartUtils'

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
/// aggregate that is a single "Bikes" series, for the nerd stats one per channel.
/// Missing weekdays are filled with 0 so the circle is always complete — this is
/// a fixed 7-slot axis, not zero-filled time buckets.
export function WeekdayRadar({
  series,
  className,
}: {
  series: RadarSeries[]
  className?: string
}) {
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
