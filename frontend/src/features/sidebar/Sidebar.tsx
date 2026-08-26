import { BarChart3, ChevronLeft, ChevronRight } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { StationListItem } from '../stations/StationListItem'
import type { StationSummary, StationSummarySidebar } from '../stations/types'

/// Left overlay panel listing the counting stations visible in the current map
/// bounds. Collapses to a slim vertical edge (Komoot style). The station list
/// scrolls in its own area; the "Summarize visible stations" action sits in a
/// pinned footer below it so the list stays scrollable.
export function Sidebar({
  collapsed,
  onToggle,
  sidebar,
  error,
  onSelectStation,
  onSummarize,
}: {
  collapsed: boolean
  onToggle: () => void
  sidebar: StationSummarySidebar | null
  error: boolean
  onSelectStation: (station: StationSummary) => void
  onSummarize: () => void
}) {
  if (collapsed) {
    return (
      <aside className="absolute inset-y-0 left-0 z-[500] flex w-10 min-h-0 flex-col bg-primary shadow-md">
        <Button
          type="button"
          variant="ghost"
          onClick={onToggle}
          title="Show station list (H)"
          aria-label="Show station list"
          className="flex-1 rounded-none text-primary-foreground hover:bg-white/15 hover:text-primary-foreground"
        >
          <ChevronRight />
        </Button>
      </aside>
    )
  }

  return (
    <aside className="absolute inset-y-0 left-0 z-[500] flex w-[360px] min-h-0 flex-col border-r bg-background shadow-lg transition-[width]">
      <div className="flex items-center justify-between gap-2 border-b px-4 py-3">
        <h2 className="min-w-0 flex-1 text-base font-semibold">Visible counting stations</h2>
        <div className="flex items-center gap-2">
          <Badge variant="default" className="rounded-full">
            {sidebar ? `${sidebar.visible_count} / ${sidebar.total_count}` : '–'}
          </Badge>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onToggle}
            title="Hide station list (H)"
            aria-label="Hide station list"
          >
            <ChevronLeft />
          </Button>
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <ul className="list-none">
          {error && (
            <li className="p-4 text-sm font-semibold text-destructive">
              Could not load counting stations.
            </li>
          )}
          {!error && sidebar === null && (
            <li className="p-4 text-sm text-muted-foreground">Loading counting stations…</li>
          )}
          {!error && sidebar !== null && sidebar.items.length === 0 && (
            <li className="p-4 text-sm text-muted-foreground">
              No counting stations visible in this area.
            </li>
          )}
          {!error &&
            (sidebar?.items ?? []).map((station) => (
              <StationListItem
                key={station.id}
                station={station}
                onSelect={onSelectStation}
                showFind={false}
              />
            ))}
        </ul>
      </ScrollArea>
      <div className="shrink-0 border-t p-3">
        <Button
          type="button"
          className="w-full"
          onClick={onSummarize}
          disabled={!sidebar || sidebar.items.length === 0}
          title="Open the aggregated summary of the visible stations"
        >
          <BarChart3 />
          Summarize visible stations
        </Button>
      </div>
    </aside>
  )
}
