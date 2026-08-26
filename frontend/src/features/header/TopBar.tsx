import { Search } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { useGlobalSummary } from './useGlobalSummary'

/// Top header bar: brand, centered search trigger, and the global summary.
export function TopBar({ onOpenSearch }: { onOpenSearch: () => void }) {
  const { summary, error } = useGlobalSummary()

  return (
    <header className="grid grid-cols-[1fr_auto_1fr] items-center gap-4 bg-primary px-4 py-2 text-primary-foreground shadow-md">
      <div className="flex items-center gap-2 font-bold whitespace-nowrap justify-self-start">
        <img
          src="/bike-icon.svg"
          alt=""
          aria-hidden="true"
          className="h-8 w-8"
        />
        <span>Bike Counter</span>
      </div>

      <Button
        type="button"
        variant="ghost"
        onClick={onOpenSearch}
        className="w-[26rem] max-w-[60vw] justify-start gap-2 rounded-lg bg-white/15 px-3 py-2 text-left font-normal text-primary-foreground hover:bg-white/25 hover:text-primary-foreground"
      >
        <Search aria-hidden="true" />
        Search counting stations…
      </Button>

      <div className="flex items-center gap-3 justify-self-end">
        {summary && (
          <span className="text-sm whitespace-nowrap text-primary-foreground/80">
            {summary.station_count} stations · {formatNumber(summary.channel_count)}{' '}
            channels · {formatNumber(summary.bikes_last_day_total)} bikes / last day · updated{' '}
            {formatTimestamp(summary.last_update)}
          </span>
        )}
        {error && <span className="text-sm text-red-200">Global summary unavailable.</span>}
      </div>
    </header>
  )
}
