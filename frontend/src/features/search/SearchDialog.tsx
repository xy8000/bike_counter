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
    <div className="dialog-overlay" onClick={onClose}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Search counting stations"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="dialog-header">
          <input
            autoFocus
            type="text"
            placeholder="Filter stations by name or description…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          {query !== '' && (
            <button
              type="button"
              className="dialog-clear"
              onClick={() => setQuery('')}
              aria-label="Clear filter"
              title="Clear filter"
            >
              ✕
            </button>
          )}
          <button type="button" className="dialog-close" onClick={onClose} aria-label="Close search">
            Close
          </button>
        </div>
        <ul className="station-list">
          {error && <li className="state error">Could not load stations.</li>}
          {!error && loading && <li className="state">Loading stations…</li>}
          {!error && !loading && results.length === 0 && (
            <li className="state">No stations match your search.</li>
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
      </div>
    </div>
  )
}
