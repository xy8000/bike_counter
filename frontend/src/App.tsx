import { Route, Routes } from 'react-router-dom'
import MapPage from './features/map/MapPage'
import { StationDetail } from './features/stationDetail/StationDetail'

/// Route table. The map is the app root; `/stations/:stationId` is the (for now
/// blank) counting-station detail page whose id lives in the URL path.
export default function App() {
  return (
    <Routes>
      <Route path="/" element={<MapPage />} />
      <Route path="/stations/:stationId" element={<StationDetail />} />
    </Routes>
  )
}
