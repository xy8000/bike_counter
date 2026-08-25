import { LocateFixed } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { formatNumber } from '../../lib/format'
import type { StationSummary } from './types'

/// A shared list entry used by both the sidebar and the search dialog. When
/// `showFind` is set, a "find on map" button is rendered next to the entry.
export function StationListItem({
  station,
  onSelect,
  onFind,
  showFind,
}: {
  station: StationSummary
  onSelect: (station: StationSummary) => void
  onFind?: (station: StationSummary) => void
  showFind: boolean
}) {
  const findable = station.latitude !== null && station.longitude !== null
  return (
    <li className="flex items-stretch border-b">
      <Button
        type="button"
        variant="ghost"
        onClick={() => onSelect(station)}
        className="flex h-auto min-w-0 flex-1 flex-col items-start justify-start gap-0.5 rounded-none px-4 py-3 text-left"
      >
        <span className="text-sm font-semibold text-foreground">{station.name}</span>
        <span className="w-full truncate text-sm text-muted-foreground">{station.description}</span>
        <span className="mt-0.5 flex w-full flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
          <span>{station.channel_count} channels</span>
          <span aria-hidden="true">·</span>
          <span>
            <span className="font-medium text-foreground">
              {formatNumber(station.bikes_last_24h)}
            </span>{' '}
            bikes / 24 h
          </span>
        </span>
      </Button>
      {showFind && onFind && findable && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onFind(station)}
          className="my-auto mr-3 shrink-0"
        >
          <LocateFixed />
          Find on map
        </Button>
      )}
    </li>
  )
}
