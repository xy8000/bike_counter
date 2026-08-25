import { Search, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { StationListItem } from '../stations/StationListItem'
import type { StationSummary } from '../stations/types'
import { useStationSearch } from '../stations/useStationSearch'

/// Modal dialog for searching counting stations by name or description.
export function SearchDialog({
  onClose,
  onSelect,
  onFind,
}: {
  onClose: () => void
  onSelect: (station: StationSummary) => void
  onFind: (station: StationSummary) => void
}) {
  const { query, setQuery, results, loading, error, findOnMapEnabled } = useStationSearch()

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        className="top-[5rem] flex max-h-[70vh] flex-col gap-0 overflow-hidden p-0 translate-y-0 sm:max-w-[560px]"
        showCloseButton={false}
      >
        <DialogHeader className="sr-only">
          <DialogTitle>Search counting stations</DialogTitle>
          <DialogDescription>Filter counting stations by name or description.</DialogDescription>
        </DialogHeader>
        <div className="flex shrink-0 items-center gap-2 border-b p-3">
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              autoFocus
              type="text"
              placeholder="Filter stations by name or description…"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              className="pl-9"
            />
          </div>
          {query !== '' && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => setQuery('')}
              aria-label="Clear filter"
              title="Clear filter"
            >
              <X />
            </Button>
          )}
          <Button type="button" variant="ghost" onClick={onClose}>
            Close
          </Button>
        </div>
        <ScrollArea className="min-h-0 flex-1">
          <ul className="list-none">
            {error && (
              <li className="p-4 text-sm font-semibold text-destructive">Could not load stations.</li>
            )}
            {!error && loading && (
              <li className="p-4 text-sm text-muted-foreground">Loading stations…</li>
            )}
            {!error && !loading && results.length === 0 && (
              <li className="p-4 text-sm text-muted-foreground">No stations match your search.</li>
            )}
            {!error &&
              results.map((station) => (
                <StationListItem
                  key={station.id}
                  station={station}
                  onSelect={onSelect}
                  onFind={onFind}
                  showFind={findOnMapEnabled}
                />
              ))}
          </ul>
        </ScrollArea>
      </DialogContent>
    </Dialog>
  )
}
