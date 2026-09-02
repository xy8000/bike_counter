import { BarChart3, X } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Skeleton } from '@/components/ui/skeleton'
import type { SidebarShell, SidebarStation, SidebarStationStats } from '../stations/types'
import { SidebarListItem } from './SidebarListItem'

/// Skeleton rows shown while the sidebar shell loads (the UI without content).
function SidebarListSkeleton({ count = 5 }: { count?: number }) {
  return (
    <ul className="list-none" aria-busy="true">
      {Array.from({ length: count }, (_, index) => (
        <li key={index} className="flex items-center gap-3 border-b px-4 py-3">
          <Skeleton className="h-12 w-12 shrink-0 rounded-md" />
          <span className="min-w-0 flex-1">
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="mt-2 h-3 w-full" />
            <Skeleton className="mt-2 h-3 w-1/2" />
          </span>
        </li>
      ))}
    </ul>
  )
}

/// The station-list content shown inside the generic left panel ([`LeftPanel`]).
/// It lists the counting stations visible in the current map bounds; the list
/// scrolls in its own area and the "Summarize visible stations" action sits in
/// a pinned footer below it.
export function Sidebar({
  shell,
  stats,
  loading,
  error,
  statsError,
  onSelectStation,
  onSummarize,
  onClose,
}: {
  shell: SidebarShell | null
  stats: Map<string, SidebarStationStats> | null
  loading: boolean
  error: boolean
  statsError: boolean
  onSelectStation: (station: SidebarStation) => void
  onSummarize: () => void
  // Phone-only affordance: the full-screen drawer gets its own close button,
  // while tablet/desktop close via the mid-height handle.
  onClose?: () => void
}) {
  return (
    <>
      <div className="flex items-center justify-between gap-2 border-b px-4 py-3">
        <h2 className="min-w-0 flex-1 text-base font-semibold">Visible counting stations</h2>
        <div className="flex shrink-0 items-center gap-2">
          <Badge variant="default" className="rounded-full">
            {shell ? `${shell.visible_count} / ${shell.total_count}` : '–'}
          </Badge>
          {onClose && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={onClose}
              title="Close station list"
              aria-label="Close station list"
              className="sm:hidden"
            >
              <X />
            </Button>
          )}
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        {error && (
          <p className="p-4 text-sm font-semibold text-destructive">
            Could not load counting stations.
          </p>
        )}
        {!error && loading && shell === null && <SidebarListSkeleton />}
        {!error && !loading && shell !== null && shell.items.length === 0 && (
          <p className="p-4 text-sm text-muted-foreground">
            No counting stations visible in this area.
          </p>
        )}
        {!error && (shell?.items ?? []).length > 0 && (
          <ul className="list-none">
            {(shell?.items ?? []).map((station) => (
              <SidebarListItem
                key={station.id}
                station={station}
                stats={stats?.get(station.id)}
                onSelect={onSelectStation}
              />
            ))}
          </ul>
        )}
        {!error && shell !== null && stats === null && !statsError && (
          <p className="sr-only" aria-live="polite">
            Loading station statistics…
          </p>
        )}
        {!error && statsError && (
          <p className="p-4 text-sm text-muted-foreground">Could not load station statistics.</p>
        )}
      </ScrollArea>
      <div className="shrink-0 border-t p-3">
        <Button
          type="button"
          className="w-full"
          onClick={onSummarize}
          disabled={!shell || shell.items.length === 0}
          title="Open the aggregated summary of the visible stations"
        >
          <BarChart3 />
          Summarize visible stations
        </Button>
      </div>
    </>
  )
}
