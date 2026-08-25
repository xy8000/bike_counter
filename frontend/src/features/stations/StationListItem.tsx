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
    <li className="station-item">
      <button type="button" className="station-item-main" onClick={() => onSelect(station)}>
        <span className="station-name">{station.name}</span>
        <span className="station-description">{station.description}</span>
        <span className="station-meta">
          {station.channel_count} channels · {formatNumber(station.bikes_last_24h)} bikes / 24 h
        </span>
      </button>
      {showFind && onFind && findable && (
        <button type="button" className="station-find" onClick={() => onFind(station)}>
          Find on map
        </button>
      )}
    </li>
  )
}
