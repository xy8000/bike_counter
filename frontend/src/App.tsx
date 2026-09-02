import { Route, Routes } from 'react-router-dom'
import MapPage from './features/map/MapPage'
import { TrendSettingsProvider } from './features/settings/TrendSettingsContext'
import { StationDetail } from './features/stationDetail/StationDetail'
import { StationsSummary } from './features/stationsSummary/StationsSummary'
import { DataSourceDetail } from './features/dataSources/DataSourceDetail'
import { DataSourcesList } from './features/dataSources/DataSourcesList'

/// Route table. The map is the app root; `/stations/:stationId` is the
/// counting-station detail page and `/summary` aggregates the currently visible
/// stations. Both carry their view state in the URL. The Bike-Trends settings
/// are app-global, so the provider wraps every route.
export default function App() {
  return (
    <TrendSettingsProvider>
      <Routes>
        <Route path="/" element={<MapPage />} />
        <Route path="/stations/:stationId" element={<StationDetail />} />
        <Route path="/summary" element={<StationsSummary />} />
        <Route path="/data-sources" element={<DataSourcesList />} />
        <Route path="/data-sources/:dataSourceId" element={<DataSourceDetail />} />
      </Routes>
    </TrendSettingsProvider>
  )
}
