import { Search } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { useGlobalSummary } from './useGlobalSummary'

/// Top header bar: brand (links home), centered search trigger, and the global
/// summary. The header stays on a single line; the side columns can shrink so
/// the summary never wraps or overflows — when space runs out the stats part
/// truncates and only the "updated …" timestamp remains.
export function TopBar({ onOpenSearch }: { onOpenSearch: () => void }) {
  const { summary, error } = useGlobalSummary()

  return (
    <header className="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-4 bg-primary px-4 py-2 text-primary-foreground shadow-md">
      <Link
        to="/"
        className="flex min-w-0 items-center gap-2 font-bold whitespace-nowrap justify-self-start"
      >
        <img
          src="/bike-icon.svg"
          alt=""
          aria-hidden="true"
          className="h-8 w-8 shrink-0"
        />
        <span className="truncate">Bike Counter</span>
      </Link>

      <Button
        type="button"
        variant="ghost"
        onClick={onOpenSearch}
        className="w-[32rem] max-w-[60vw] justify-start gap-2 rounded-lg bg-white/15 px-3 py-2 text-left font-normal text-primary-foreground hover:bg-white/25 hover:text-primary-foreground"
      >
        <Search aria-hidden="true" />
        Search counting stations…
      </Button>

      <div className="flex min-w-0 items-center justify-end justify-self-end overflow-hidden">
        {summary && (
          <span className="flex min-w-0 items-center gap-1 text-sm text-primary-foreground/80">
            {/* The stats truncate first; the update timestamp stays visible when
                there is not enough space, per plan 55. */}
            <span className="truncate">
              {summary.station_count} stations · {formatNumber(summary.channel_count)} channels ·{' '}
              {formatNumber(summary.bikes_last_day_total)} bikes / last day
            </span>
            <span className="shrink-0 whitespace-nowrap">
              updated {formatTimestamp(summary.last_update)}
            </span>
          </span>
        )}
        {error && (
          <span className="truncate text-sm text-red-200">Global summary unavailable.</span>
        )}
      </div>
    </header>
  )
}
