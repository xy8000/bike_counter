import { formatNumber, formatTimestamp } from '../../lib/format'
import { useGlobalSummary } from './useGlobalSummary'

/// Top header bar: brand, centered search trigger, and the global summary.
export function TopBar({ onOpenSearch }: { onOpenSearch: () => void }) {
  const { summary, error } = useGlobalSummary()

  return (
    <header className="topbar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true">
          🚴
        </span>
        <span className="brand-name">Bike Counter</span>
      </div>

      <button type="button" className="search-trigger" onClick={onOpenSearch}>
        <span aria-hidden="true">🔍</span> Search counting stations…
      </button>

      <div className="topbar-right">
        {summary && (
          <span className="global-summary">
            {summary.station_count} stations · {formatNumber(summary.channel_count)}{' '}
            channels · {formatNumber(summary.bikes_last_24h_total)} bikes / 24 h · updated{' '}
            {formatTimestamp(summary.last_update)}
          </span>
        )}
        {error && <span className="global-summary error">Global summary unavailable.</span>}
      </div>
    </header>
  )
}
