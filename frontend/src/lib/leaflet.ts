import L from 'leaflet'
import iconRetinaUrl from 'leaflet/dist/images/marker-icon-2x.png'
import iconUrl from 'leaflet/dist/images/marker-icon.png'
import shadowUrl from 'leaflet/dist/images/marker-shadow.png'
import 'leaflet/dist/leaflet.css'
// The brand emerald (--primary, emerald-600 #059669) pin-with-bike marker,
// imported as a Vite asset so editing the SVG file in the map feature updates
// the markers on rebuild/HMR without a code change.
import markerUrl from '../features/map/map-flag-counting-station.svg'

// Leaflet's default icon points at bare asset names; Vite bundles the images,
// so point the default icon at the resolved URLs explicitly.
// This module is imported for its side effect (once, at first import).
L.Icon.Default.mergeOptions({
  iconRetinaUrl,
  iconUrl,
  shadowUrl,
})

// Map marker: the brand pin + bike at its native 32x40 size, anchored on the
// bottom tip of the teardrop. Keeps the default marker shadow so the popup
// positioning and the visual depth stay as before.
export const stationIcon = L.icon({
  iconUrl: markerUrl,
  iconSize: [32, 40],
  iconAnchor: [16, 37],
  popupAnchor: [1, -32],
  shadowUrl,
  shadowSize: [41, 41],
})

// Larger, "highlighted" variant used on the detail page's map preview so the
// single station stands out. Same brand pin-and-bike asset, rendered scaled up
// (the SVG is a vector, so it stays sharp).
export const detailStationIcon = L.icon({
  iconUrl: markerUrl,
  iconSize: [44, 55],
  iconAnchor: [22, 51],
  popupAnchor: [0, -45],
  shadowUrl,
  shadowSize: [41, 41],
})

// Grayed-out variant used on the station-summary page for disabled stations
// (the user clicked their flag to exclude them from the charts). The emerald
// pin is turned into a muted silhouette via the `leaflet-disabled-marker` CSS
// class (see index.css), so no second SVG is needed.
export const disabledStationIcon = L.icon({
  iconUrl: markerUrl,
  iconSize: [32, 40],
  iconAnchor: [16, 37],
  popupAnchor: [1, -32],
  shadowUrl,
  shadowSize: [41, 41],
  className: 'leaflet-disabled-marker',
})
