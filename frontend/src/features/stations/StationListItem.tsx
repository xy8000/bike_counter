import { FileText, LocateFixed } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { formatNumber } from '../../lib/format'
import type { StationSummary } from './types'

/// A shared list entry used by both the sidebar and the search dialog. The row
/// shows the station image thumbnail (the built-in bike icon when the station
/// has no provider image) next to the identity; when `showFind` is set, a "find
/// on map" button is rendered next to the entry; when `showDetail` is set, an
/// "open detail" button opens the station's detail page.
export function StationListItem({
  station,
  onSelect,
  onFind,
  onDetail,
  showFind,
  showDetail,
}: {
  station: StationSummary
  onSelect: (station: StationSummary) => void
  onFind?: (station: StationSummary) => void
  onDetail?: (station: StationSummary) => void
  showFind: boolean
  showDetail?: boolean
}) {
  const findable = station.latitude !== null && station.longitude !== null
  // The row wraps so the action buttons move below the station text on narrow
  // widths instead of overflowing the layout; on sm+ everything stays on one
  // line with the text truncating.
  return (
    <li className="flex flex-wrap items-stretch border-b transition-colors hover:bg-accent sm:flex-nowrap">
      <Button
        type="button"
        variant="ghost"
        onClick={() => onSelect(station)}
        className="flex h-auto min-w-0 w-full items-center gap-3 rounded-none px-4 py-3 text-left hover:bg-transparent sm:flex-1"
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
          <span className="mt-0.5 flex w-full flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
            <span>{station.channel_count} channels</span>
            <span aria-hidden="true">·</span>
            <span>
              <span className="font-medium text-foreground">
                {formatNumber(station.bikes_last_day)}
              </span>{' '}
              bikes / last day
            </span>
          </span>
        </span>
      </Button>
      {showFind && onFind && findable && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onFind(station)}
          aria-label="Find on map"
          title="Find on map"
          className="my-1 ml-auto mr-1 shrink-0 sm:my-auto sm:ml-0 sm:mr-2"
        >
          <LocateFixed />
          <span className="hidden sm:inline">Find on map</span>
        </Button>
      )}
      {showDetail && onDetail && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onDetail(station)}
          aria-label="Open detail"
          title="Open detail"
          className="my-1 mr-3 shrink-0 sm:my-auto"
        >
          <FileText />
          <span className="hidden sm:inline">Open detail</span>
        </Button>
      )}
    </li>
  )
}
