import { Cell, Pie, PieChart } from 'recharts'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import type { ChannelRef, ChannelTotal } from './types'
import { ChartEmptyState } from './ChartEmptyState'
import { seriesColor } from './chartUtils'

/// Donut of each channel's share over the selected timeframe. Only channels with
/// traffic get a slice; the tooltip shows the channel name. A small legend lists
/// every slice with its colour and total.
export function ChannelPie({
  totals,
  channels,
  className,
}: {
  totals: ChannelTotal[]
  channels: ChannelRef[]
  className?: string
}) {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  const data = totals
    .filter((total) => total.total > 0)
    .map((total) => ({
      channel_id: total.channel_id,
      name: nameOf(total.channel_id),
      total: total.total,
    }))
  const config: ChartConfig = { total: { label: 'Bikes' } }

  return (
    <div className="flex flex-col items-center gap-3">
      {data.length === 0 ? (
        <ChartEmptyState message="No traffic for this period." className="py-16" />
      ) : (
        <>
          <ChartContainer
            config={config}
            className={cn('aspect-square w-full max-w-[280px]', className)}
          >
            <PieChart>
              <ChartTooltip content={<ChartTooltipContent nameKey="name" />} />
              <Pie
                data={data}
                dataKey="total"
                nameKey="name"
                innerRadius={45}
                outerRadius={90}
                paddingAngle={2}
                isAnimationActive={false}
              >
                {data.map((entry, index) => (
                  <Cell key={entry.name} fill={seriesColor(index)} />
                ))}
              </Pie>
            </PieChart>
          </ChartContainer>
          <ul className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1">
            {data.map((entry, index) => (
              <li
                key={entry.name}
                className="flex items-center gap-1.5 text-xs text-muted-foreground"
              >
                <span
                  className="h-2.5 w-2.5 rounded-[2px]"
                  style={{ backgroundColor: seriesColor(index) }}
                />
                {entry.name} · {entry.total}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}
