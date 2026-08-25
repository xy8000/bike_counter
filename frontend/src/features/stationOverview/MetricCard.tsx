import { formatNumber } from '../../lib/format'
import { TrendIcon } from './TrendIcon'
import type { StationOverviewMetric } from './types'

/// Human-readable labels for every overview metric key, including the YEAR stat
/// that only appears on the detail page.
export const METRIC_LABELS: Record<string, string> = {
  last_day: 'Last 24 hours',
  last_7_days: 'Last 7 days',
  last_month: 'Last month',
  last_year: 'Last year',
}

/// One overview stat box: label, current total and the trend vs. the previous
/// period. Shared between the overview panel and the detail page so the small
/// boxes look identical everywhere (the detail page just adds the YEAR stat).
export function MetricCard({ metric }: { metric: StationOverviewMetric }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border p-3">
      <div className="min-w-0">
        <p className="text-sm font-medium">{METRIC_LABELS[metric.key] ?? metric.key}</p>
        <p className="text-2xl font-semibold leading-tight">
          {formatNumber(metric.current)}
          <span className="ml-1 text-xs font-normal text-muted-foreground">bikes</span>
        </p>
      </div>
      <div className="flex shrink-0 flex-col items-end gap-0.5">
        <div className="flex items-center gap-1">
          <TrendIcon trend={metric.trend} />
          <span className="text-sm font-semibold">
            {metric.delta_percent === null
              ? '–'
              : `${metric.delta_percent > 0 ? '+' : ''}${metric.delta_percent}%`}
          </span>
        </div>
        <span className="text-xs text-muted-foreground">
          vs. {formatNumber(metric.previous)}
        </span>
      </div>
    </div>
  )
}
