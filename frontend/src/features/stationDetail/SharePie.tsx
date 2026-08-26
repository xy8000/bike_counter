import { Cell, Pie, PieChart } from 'recharts'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { cn } from '@/lib/utils'
import { ChartEmptyState } from './ChartEmptyState'
import { seriesColor } from './chartUtils'

/// One share slice: an id, the display name and the total. Used for both the
/// detail page's channel pie and the summary page's per-station pie.
export interface ShareSlice {
  id: string
  name: string
  total: number
}

/// Donut of each slice's share over the selected timeframe. Only slices with
/// traffic get a segment; the tooltip shows the slice name. A small legend lists
/// every slice with its colour and total.
export function SharePie({
  slices,
  className,
}: {
  slices: ShareSlice[]
  className?: string
}) {
  const data = slices.filter((slice) => slice.total > 0)
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
