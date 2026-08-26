import { Skeleton } from '@/components/ui/skeleton'
import { formatNumber } from '../../lib/format'
import type { SidebarStation, SidebarStationStats } from '../stations/types'

/// A sidebar list entry: the station identity (image + name + description)
/// renders directly from the shell, and the stats line shows a skeleton until
/// the stats sub-resource arrives. The search dialog keeps its own
/// `StationListItem` (no image, inline stats) — this one is sidebar-only.
export function SidebarListItem({
  station,
  stats,
  onSelect,
}: {
  station: SidebarStation
  stats?: SidebarStationStats
  onSelect: (station: SidebarStation) => void
}) {
  return (
    <li className="flex flex-wrap items-stretch border-b sm:flex-nowrap">
      <button
        type="button"
        onClick={() => onSelect(station)}
        className="flex h-auto w-full min-w-0 cursor-pointer items-center gap-3 rounded-none px-4 py-3 text-left sm:flex-1"
      >
        <img
          src={station.image_url}
          alt=""
          className="h-12 w-12 shrink-0 rounded-md border object-cover"
        />
        <span className="min-w-0 flex-1">
          <span className="block w-full truncate text-sm font-semibold text-foreground">
            {station.name}
          </span>
          <span className="block w-full truncate text-sm text-muted-foreground">
            {station.description}
          </span>
          {stats ? (
            <span className="mt-0.5 flex w-full flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
              <span>
                {stats.channel_count} channel
                {stats.channel_count === 1 ? '' : 's'}
              </span>
              <span aria-hidden="true">·</span>
              <span>
                <span className="font-medium text-foreground">
                  {formatNumber(stats.bikes_last_day)}
                </span>{' '}
                bikes / last day
              </span>
            </span>
          ) : (
            <span className="mt-1 flex items-center gap-2">
              <Skeleton className="h-3 w-12" />
              <Skeleton className="h-3 w-20" />
            </span>
          )}
        </span>
      </button>
    </li>
  )
}
