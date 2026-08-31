import { ExternalLink, X } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Skeleton } from '@/components/ui/skeleton'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { MetricCard } from './MetricCard'
import { OverviewPanelSkeleton } from './Skeletons'
import { TotalBikesCard } from './TotalBikesCard'
import { useStationOverview } from './useStationOverview'

/// The counting-station overview content shown inside the generic left panel
/// ([`LeftPanel`]) when a map marker is selected; clicking the map void or the
/// close button returns to the station list. The shell (identity — the name
/// renders immediately) loads first; the stats card fills in from the parallel
/// stats sub-resource.
export function StationOverview({
  stationId,
  onClose,
}: {
  stationId: string
  onClose: () => void
}) {
  const { page, stats, error, statsError } = useStationOverview(stationId)

  return (
    <>
      <div className="border-b px-4 py-3">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1">
            {page ? (
              // The station name opens the detail page without looking like a
              // link (heading styling) and stays in the same tab; the icon
              // button is the explicit affordance.
              <Link
                to={page.detail_url}
                className="block min-w-0 truncate text-base font-semibold text-foreground hover:no-underline"
              >
                {page.name}
              </Link>
            ) : (
              <h2 className="min-w-0 truncate text-base font-semibold">Counting station</h2>
            )}
            {page && (
              <>
                {page.description && (
                  <p className="mt-0.5 text-xs text-muted-foreground">{page.description}</p>
                )}
                <Badge variant="secondary" className="mt-1">
                  {formatNumber(page.channel_count)} channel
                  {page.channel_count === 1 ? '' : 's'}
                </Badge>
              </>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {page && (
              <Button
                asChild
                variant="ghost"
                size="icon"
                title="Open detail page"
                aria-label="Open detail page"
              >
                <Link to={page.detail_url}>
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
      </div>
      <ScrollArea className="min-h-0 flex-1">
        {error && (
          <p className="p-4 text-sm font-semibold text-destructive">
            Could not load the station overview.
          </p>
        )}
        {!error && page === null && (
          <div className="flex flex-col gap-4 p-4" aria-busy="true">
            <Skeleton className="h-40 w-full rounded-md" />
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-4 w-1/2" />
            <div className="flex items-center justify-between gap-2">
              <Skeleton className="h-5 w-16 rounded-md" />
              <Skeleton className="h-4 w-24" />
            </div>
            <OverviewPanelSkeleton />
          </div>
        )}
        {!error && page && (
          <div className="flex flex-col gap-4 p-4">
            <img
              src={page.image_url}
              alt={`${page.name} image`}
              className="h-40 w-full rounded-md border object-cover"
            />
            <div className="flex items-center justify-end">
              <span className="text-xs text-muted-foreground">
                Updated {formatTimestamp(page.last_update)}
              </span>
            </div>
            {stats ? (
              <>
                <TotalBikesCard total={stats.total_bikes} />
                <ul className="flex flex-col gap-2">
                  {stats.metrics.map((metric) => (
                    <li key={metric.key}>
                      <MetricCard metric={metric} />
                    </li>
                  ))}
                </ul>
              </>
            ) : statsError ? (
              <p className="text-sm font-semibold text-destructive">
                Could not load the overview stats.
              </p>
            ) : (
              <div className="flex flex-col gap-4" aria-busy="true">
                <OverviewPanelSkeleton />
              </div>
            )}
          </div>
        )}
      </ScrollArea>
    </>
  )
}
