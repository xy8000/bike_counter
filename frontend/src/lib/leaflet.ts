import L from 'leaflet'
import iconRetinaUrl from 'leaflet/dist/images/marker-icon-2x.png'
import iconUrl from 'leaflet/dist/images/marker-icon.png'
import shadowUrl from 'leaflet/dist/images/marker-shadow.png'
import 'leaflet/dist/leaflet.css'

// Leaflet's default icon points at bare asset names; Vite bundles the images,
// so point the default icon at the resolved URLs explicitly.
// This module is imported for its side effect (once, at first import).
L.Icon.Default.mergeOptions({
  iconRetinaUrl,
  iconUrl,
  shadowUrl,
})

// Emerald (the app's --primary, emerald-600 #059669) teardrop pin matching the
// header bar, so map markers share the brand colour instead of Leaflet's
// default blue. Keeps the default icon geometry (25x41, anchor bottom tip) and
// shadow so popup positioning stays identical.
const markerSvg = `
<svg xmlns="http://www.w3.org/2000/svg" width="25" height="41" viewBox="0 0 25 41">
  <path fill="#059669" stroke="#ffffff" stroke-width="1.5" d="M12.5 0C5.6 0 0 5.6 0 12.5c0 9.2 12.5 28.5 12.5 28.5S25 21.7 25 12.5C25 5.6 19.4 0 12.5 0z"/>
  <circle cx="12.5" cy="12.5" r="5.5" fill="#ffffff"/>
</svg>`.trim()

export const stationIcon = L.icon({
  iconUrl: `data:image/svg+xml;utf8,${encodeURIComponent(markerSvg)}`,
  iconSize: [25, 41],
  iconAnchor: [12, 41],
  popupAnchor: [1, -34],
  shadowUrl,
  shadowSize: [41, 41],
})

// Larger, "highlighted" variant used on the detail page's map preview: a thicker
// emerald teardrop with a soft halo so the single station stands out. Same brand
// colour (emerald-600 #059669) and styling language as `stationIcon`.
const detailMarkerSvg = `
<svg xmlns="http://www.w3.org/2000/svg" width="34" height="54" viewBox="0 0 34 54">
  <circle cx="17" cy="16" r="15" fill="#059669" fill-opacity="0.2"/>
  <path fill="#059669" stroke="#ffffff" stroke-width="2" d="M17 2C9.3 2 3 8.3 3 16c0 11.4 14 34 14 34s14-22.6 14-34C31 8.3 24.7 2 17 2z"/>
  <circle cx="17" cy="16" r="6.5" fill="#ffffff"/>
</svg>`.trim()

export const detailStationIcon = L.icon({
  iconUrl: `data:image/svg+xml;utf8,${encodeURIComponent(detailMarkerSvg)}`,
  iconSize: [34, 54],
  iconAnchor: [17, 54],
  popupAnchor: [0, -46],
  shadowUrl,
  shadowSize: [41, 41],
})
