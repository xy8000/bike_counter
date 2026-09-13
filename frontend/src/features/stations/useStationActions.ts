import { useNavigate } from 'react-router-dom'
import { serializeBounds, stationBounds } from '../../lib/geo'
import type { StationSummary } from './types'

/// The two station navigation actions every page header needs: open a station's
/// detail page in the same tab, or return to the map centred on the station.
export function useStationActions() {
  const navigate = useNavigate()

  const openDetail = (station: StationSummary) => {
    navigate(`/stations/${station.id}`)
  }

  // "Find on map": return to the map and fly to the station by seeding a small
  // bbox around it (plus the open-overview param). Stations without coordinates
  // just open the map at the station id.
  const findOnMap = (station: StationSummary) => {
    if (station.latitude !== null && station.longitude !== null) {
      const params = serializeBounds(stationBounds(station.latitude, station.longitude))
      params.set('station', station.id)
      navigate(`/?${params.toString()}`)
    } else {
      navigate(`/?station=${station.id}`)
    }
  }

  return { openDetail, findOnMap }
}
