import 'maplibre-gl/dist/maplibre-gl.css'
// maplibre-gl v6 runs its tile parsing in a separate Web Worker
// (`maplibre-gl-worker.mjs`). The bundler does not emit that worker on its own,
// so without this the worker 404s, no tiles are fetched and the map's `load`
// event never fires. Bundle the worker with Vite (`?worker&url`) and register
// its URL before any map is created.
import { addProtocol, setWorkerUrl } from 'maplibre-gl'
import maplibreWorkerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import { Protocol } from 'pmtiles'
// The three station flags, imported as Vite assets so editing the SVG files in
// the map feature updates the markers on rebuild/HMR without a code change.
import activeFlagUrl from '../features/map/station-flag.svg'
import selectedFlagUrl from '../features/map/station-flag-selected.svg'
import inactiveFlagUrl from '../features/map/station-flag-inactive.svg'

setWorkerUrl(maplibreWorkerUrl)

// Registers the `pmtiles://` protocol so MapLibre reads vector tiles directly
// out of a static `.pmtiles` file via HTTP range requests, with no tile-server
// process (see frontend/public/styles/basemap.json and tiles/README.md).
addProtocol('pmtiles', new Protocol().tile)

export { activeFlagUrl, selectedFlagUrl, inactiveFlagUrl }

/// The flag a station marker renders. `selected` is derived from the URL's
/// `station` param (which station the user opened); `active`/`inactive` come
/// from the BFF-reported station status.
export type StationMarkerState = 'active' | 'selected' | 'inactive'

function flagUrl(state: StationMarkerState): string {
  switch (state) {
    case 'selected':
      return selectedFlagUrl
    case 'inactive':
      return inactiveFlagUrl
    default:
      return activeFlagUrl
  }
}

/// Builds the DOM marker image used by the MapLibre markers on all three maps.
/// `alt`/`title` carry the station name: the Playwright e2e tests locate a
/// marker and read its name from `alt`, and it improves accessibility. The
/// state classes let the e2e tests assert the active/selected/inactive flag;
/// the disabled variant (station-summary page) gets the grayscale silhouette.
export function stationMarkerImage(
  name: string,
  options: { state?: StationMarkerState; disabled?: boolean; large?: boolean } = {},
) {
  const state = options.state ?? 'active'
  const classes = ['station-marker']
  if (state === 'selected') classes.push('station-marker--selected')
  if (state === 'inactive') classes.push('station-marker--inactive')
  if (options.disabled) classes.push('station-marker--disabled')
  if (options.large) classes.push('station-marker--large')
  return (
    <img
      src={flagUrl(state)}
      alt={name}
      title={name}
      className={classes.join(' ')}
      draggable={false}
    />
  )
}
