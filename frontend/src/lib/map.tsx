import 'maplibre-gl/dist/maplibre-gl.css'
// maplibre-gl v6 runs its tile parsing in a separate Web Worker
// (`maplibre-gl-worker.mjs`). The bundler does not emit that worker on its own,
// so without this the worker 404s, no tiles are fetched and the map's `load`
// event never fires. Bundle the worker with Vite (`?worker&url`) and register
// its URL before any map is created.
import { addProtocol, setWorkerUrl } from 'maplibre-gl'
import maplibreWorkerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import { Protocol } from 'pmtiles'
// The brand emerald (--primary, emerald-600 #059669) pin-with-bike marker,
// imported as a Vite asset so editing the SVG file in the map feature updates
// the markers on rebuild/HMR without a code change.
import markerUrl from '../features/map/map-flag-counting-station.svg'

setWorkerUrl(maplibreWorkerUrl)

// Registers the `pmtiles://` protocol so MapLibre reads vector tiles directly
// out of a static `.pmtiles` file via HTTP range requests, with no tile-server
// process (see frontend/public/styles/basemap.json and tiles/README.md).
addProtocol('pmtiles', new Protocol().tile)

export { markerUrl }

/// Builds the DOM marker image used by the MapLibre markers on all three maps.
/// `alt`/`title` carry the station name: the Playwright e2e tests locate a
/// marker and read its name from `alt`, and it improves accessibility. The
/// disabled variant (station-summary page) gets the grayscale silhouette class.
export function stationMarkerImage(
  name: string,
  options: { disabled?: boolean; large?: boolean } = {},
) {
  const classes = ['station-marker']
  if (options.disabled) classes.push('station-marker--disabled')
  if (options.large) classes.push('station-marker--large')
  return (
    <img src={markerUrl} alt={name} title={name} className={classes.join(' ')} draggable={false} />
  )
}
