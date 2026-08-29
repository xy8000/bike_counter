import { Marker } from '@vis.gl/react-maplibre'
import type { Bounds } from '../../lib/geo'
import { stationMarkerImage } from '../../lib/map'
import { BaseMap } from '../map/BaseMap'
import type { SummaryStation } from './types'

/// The interactive map on the station-summary page, fitted to the same bounds
/// the user had selected. Clicking a station's flag toggles it disabled: the
/// marker turns gray and the station is excluded from the charts (the page
/// re-fetches with the updated `disabled` set).
export function SummaryMap({
  stations,
  disabled,
  onToggle,
  bounds,
}: {
  stations: SummaryStation[]
  disabled: Set<string>
  onToggle: (stationId: string) => void
  bounds: Bounds
}) {
  return (
    <div className="h-64 w-full overflow-hidden rounded-lg border md:h-80" aria-label="Summary map">
      <BaseMap bounds={bounds} scrollZoom={false} navigationControl>
        {stations.map((station) => {
          const isDisabled = disabled.has(station.id)
          return (
            <Marker key={station.id} longitude={station.longitude} latitude={station.latitude}>
              {/* The marker DOM element is a child of the map container, so its
                  click bubbles up to the map's onClick; stop it here. */}
              <div
                className="cursor-pointer"
                onClick={(event) => {
                  event.stopPropagation()
                  onToggle(station.id)
                }}
              >
                {stationMarkerImage(station.name, { disabled: isDisabled })}
              </div>
            </Marker>
          )
        })}
      </BaseMap>
    </div>
  )
}
