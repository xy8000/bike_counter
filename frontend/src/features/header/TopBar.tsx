import { Database, Info, Search } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { formatTimestamp } from '../../lib/format'
import { GlobalSummaryDialog } from './GlobalSummaryDialog'
import { useGlobalSummary } from './useGlobalSummary'

/// Top header bar: brand (links home), centered search trigger, and the global
/// summary. Only the `updated …` timestamp stays visible inline; it doubles as a
/// button that opens the full summary (stations, channels, bikes) in a popup
/// dialogue. While the summary loads, a skeleton of the same height stands in so
/// the bar never shifts. The right side shares a cell on phones: the compact
/// search icon sits next to the timestamp trigger.
export function TopBar({ onOpenSearch }: { onOpenSearch: () => void }) {
  const { summary, error } = useGlobalSummary()
  const [summaryOpen, setSummaryOpen] = useState(false)

  return (
    <header className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 bg-primary px-4 py-2 text-primary-foreground shadow-md sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] sm:gap-4">
      {/* Brand (links home) + data-sources shortcut. Stretches and truncates so
          the always-visible summary timestamp never gets squeezed off. */}
      <div className="flex min-w-0 items-center gap-1">
        <Link to="/" className="flex min-w-0 items-center gap-2 font-bold whitespace-nowrap">
          <img src="/bike-icon.svg" alt="" aria-hidden="true" className="h-8 w-8 shrink-0" />
          <span className="truncate">Bike Counter</span>
        </Link>
        {/* Data-sources section: an icon on phones (no room), a labelled link on
            large screens. Reachable on every page from the shared top bar. */}
        <Link
          to="/data-sources"
          aria-label="Data sources"
          title="Data sources"
          className="flex items-center gap-1 whitespace-nowrap rounded px-2 py-1 text-sm text-primary-foreground hover:bg-white/10"
        >
          <Database aria-hidden="true" className="h-4 w-4 shrink-0" />
          <span className="hidden lg:inline">Data sources</span>
        </Link>
      </div>

      {/* Wide search trigger on sm+ (the centred middle column). */}
      <Button
        type="button"
        variant="ghost"
        onClick={onOpenSearch}
        className="hidden w-[32rem] max-w-[60vw] justify-start gap-2 rounded-lg bg-white/15 px-3 py-2 text-left font-normal text-primary-foreground hover:bg-white/25 hover:text-primary-foreground sm:flex"
      >
        <Search aria-hidden="true" />
        Search counting stations…
      </Button>

      {/* Right side: the compact search icon on phones plus the always-visible
          global-summary timestamp. The timestamp opens the summary popup. */}
      <div className="flex min-w-0 items-center justify-self-end gap-1">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={onOpenSearch}
          aria-label="Search counting stations"
          title="Search counting stations"
          className="sm:hidden"
        >
          <Search aria-hidden="true" />
        </Button>

        {summary && (
          <button
            type="button"
            onClick={() => setSummaryOpen(true)}
            aria-haspopup="dialog"
            title="Show global summary"
            className="flex h-8 max-w-full items-center gap-1 rounded-md px-2 text-sm whitespace-nowrap text-primary-foreground/80 transition-colors hover:bg-white/10 hover:text-primary-foreground focus-visible:ring-2 focus-visible:ring-white/60 focus-visible:outline-none"
          >
            <Info aria-hidden="true" className="h-4 w-4 shrink-0" />
            <span className="truncate">updated {formatTimestamp(summary.last_update)}</span>
          </button>
        )}
        {error && (
          <span className="truncate text-sm text-red-200">Global summary unavailable.</span>
        )}
        {!summary && !error && <Skeleton aria-busy="true" className="h-8 w-44 bg-white/15" />}
      </div>

      {summaryOpen && summary && (
        <GlobalSummaryDialog summary={summary} onClose={() => setSummaryOpen(false)} />
      )}
    </header>
  )
}
