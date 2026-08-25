import { ExternalLink, X } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { TrendIcon } from './TrendIcon'
import { useStationOverview } from './useStationOverview'

const METRIC_LABELS: Record<string, string> = {
  last_day: 'Last 24 hours',
  last_7_days: 'Last 7 days',
  last_month: 'Last month',
}

/// The counting-station overview panel. Rendered in the same left slot as the
/// sidebar (same size/style) when a map marker is selected; clicking the map
/// void or the close button returns to the sidebar.
export function StationOverview({
  stationId,
  onClose,
}: {
  stationId: string
  onClose: () => void
}) {
  const { overview, error } = useStationOverview(stationId)

  return (
    <aside className="absolute inset-y-0 left-0 z-[500] flex w-[360px] min-h-0 flex-col border-r bg-background shadow-lg">
      <div className="flex items-center justify-between gap-2 border-b px-4 py-3">
        <h2 className="min-w-0 flex-1 truncate text-base font-semibold">
          {overview ? overview.name : 'Counting station'}
        </h2>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={onClose}
          title="Close overview (click the map)"
          aria-label="Close station overview"
        >
          <X />
        </Button>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        {error && (
          <p className="p-4 text-sm font-semibold text-destructive">
            Could not load the station overview.
          </p>
        )}
        {!error && overview === null && (
          <p className="p-4 text-sm text-muted-foreground">Loading counting station…</p>
        )}
        {!error && overview && (
          <div className="flex flex-col gap-4 p-4">
            <img
              src={overview.image_url}
              alt={`${overview.name} image`}
              className="h-40 w-full rounded-md border object-cover"
            />
            {/* Link to the future detail page (rendered as an external link). */}
            <a
              href={overview.detail_url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex w-fit items-center gap-1 text-sm font-semibold text-primary underline-offset-4 hover:underline"
            >
              Open detail page <ExternalLink className="h-3.5 w-3.5" />
            </a>
            {overview.description && (
              <p className="text-sm text-muted-foreground">{overview.description}</p>
            )}
            <div className="flex items-center justify-between gap-2">
              <Badge variant="secondary">
                {formatNumber(overview.channel_count)} channel
                {overview.channel_count === 1 ? '' : 's'}
              </Badge>
              <span className="text-xs text-muted-foreground">
                Updated {formatTimestamp(overview.last_update)}
              </span>
            </div>
            <ul className="flex flex-col gap-2">
              {overview.metrics.map((metric) => (
                <li
                  key={metric.key}
                  className="flex items-center justify-between gap-3 rounded-md border p-3"
                >
                  <div className="min-w-0">
                    <p className="text-sm font-medium">
                      {METRIC_LABELS[metric.key] ?? metric.key}
                    </p>
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
                </li>
              ))}
            </ul>
          </div>
        )}
      </ScrollArea>
    </aside>
  )
}
