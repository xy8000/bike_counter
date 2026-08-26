import { Route, Routes } from 'react-router-dom'
import MapPage from './features/map/MapPage'
import { StationDetail } from './features/stationDetail/StationDetail'
import { StationsSummary } from './features/stationsSummary/StationsSummary'

/// Route table. The map is the app root; `/stations/:stationId` is the
/// counting-station detail page and `/summary` aggregates the currently visible
/// stations. Both carry their view state in the URL.
export default function App() {
  return (
    <Routes>
      <Route path="/" element={<MapPage />} />
      <Route path="/stations/:stationId" element={<StationDetail />} />
      <Route path="/summary" element={<StationsSummary />} />
    </Routes>
  )
}
