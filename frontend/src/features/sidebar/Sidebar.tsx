import { StationListItem } from '../stations/StationListItem'
import type { StationSummary, StationSummarySidebar } from '../stations/types'

/// Left overlay panel listing the counting stations visible in the current map
/// bounds. Collapses to a slim vertical edge (Komoot style).
export function Sidebar({
  collapsed,
  onToggle,
  sidebar,
  error,
  onSelectStation,
}: {
  collapsed: boolean
  onToggle: () => void
  sidebar: StationSummarySidebar | null
  error: boolean
  onSelectStation: (station: StationSummary) => void
}) {
  if (collapsed) {
    return (
      <aside className="sidebar collapsed">
        <button
          type="button"
          className="sidebar-edge"
          onClick={onToggle}
          title="Show station list (H)"
          aria-label="Show station list"
        >
          {'>'}
        </button>
      </aside>
    )
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <h2>Visible counting stations</h2>
        <div className="sidebar-header-actions">
          <span className="count-badge">
            {sidebar ? `${sidebar.visible_count} / ${sidebar.total_count}` : '–'}
          </span>
          <button
            type="button"
            className="collapse-toggle"
            onClick={onToggle}
            title="Hide station list (H)"
            aria-label="Hide station list"
          >
            {'<'}
          </button>
        </div>
      </div>
      <ul className="station-list">
        {error && <li className="state error">Could not load counting stations.</li>}
        {!error && sidebar === null && <li className="state">Loading counting stations…</li>}
        {!error && sidebar !== null && sidebar.items.length === 0 && (
          <li className="state">No counting stations visible in this area.</li>
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
    </aside>
  )
}
