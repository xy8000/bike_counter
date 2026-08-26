import { ExternalLink, X } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { MetricCard } from './MetricCard'
import { TotalBikesCard } from './TotalBikesCard'
import { useStationOverview } from './useStationOverview'

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
        {overview ? (
          // The station name opens the detail page without looking like a link
          // (heading styling) and stays in the same tab; the icon button is the
          // explicit affordance.
          <Link
            to={`/stations/${stationId}`}
            className="min-w-0 flex-1 truncate text-base font-semibold text-foreground hover:no-underline"
          >
            {overview.name}
          </Link>
        ) : (
          <h2 className="min-w-0 flex-1 truncate text-base font-semibold">
            Counting station
          </h2>
        )}
        <div className="flex items-center gap-1">
          {overview && (
            <Button
              asChild
              variant="ghost"
              size="icon"
              title="Open detail page"
              aria-label="Open detail page"
            >
              <Link to={`/stations/${stationId}`}>
                <ExternalLink />
              </Link>
            </Button>
          )}
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
            <TotalBikesCard total={overview.total_bikes} />
            <ul className="flex flex-col gap-2">
              {overview.metrics.map((metric) => (
                <li key={metric.key}>
                  <MetricCard metric={metric} />
                </li>
              ))}
            </ul>
          </div>
        )}
      </ScrollArea>
    </aside>
  )
}
