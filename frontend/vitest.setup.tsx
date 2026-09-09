/// Vitest global setup for the frontend unit suite.
///
/// Runs once per test file (before each file's tests) in the `jsdom`
/// environment. Provides:
///   * jest-dom matchers (`toBeInTheDocument`, ...) for component assertions,
///   * DOM shims the UI stack needs under jsdom (matchMedia, ResizeObserver,
///     PointerEvent, scrollIntoView),
///   * lightweight module mocks for the browser-heavy libraries
///     (@vis.gl/react-maplibre, maplibre-gl, recharts) so feature components
///     can be rendered without a real WebGL map / canvas chart. The mocked
///     components forward props, render their children and (for the Map) fire
///     the `onLoad` callback so the surrounding view logic (viewport bounds,
///     clustering, markers) runs — which is what the 80 % line gate counts.
import '@testing-library/jest-dom/vitest'
import { afterEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'
import { useEffect, type ReactNode } from 'react'

// jsdom shims ---------------------------------------------------------------

// matchMedia: the basemap-style hook and some Radix internals read it.
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

// ResizeObserver: Radix dialog/select/tooltip shims.
class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
window.ResizeObserver = window.ResizeObserver ?? (ResizeObserverMock as never)

// PointerEvent: Radix uses pointer capture bookkeeping; jsdom lacks it.
if (!window.PointerEvent) {
  window.PointerEvent = MouseEvent as never
}

// scrollIntoView / scrollTo are not implemented by jsdom.
Element.prototype.scrollIntoView = Element.prototype.scrollIntoView ?? (() => {})
window.scrollTo = window.scrollTo ?? (() => {})
Element.prototype.scrollTo = Element.prototype.scrollTo ?? (() => {})

/// A stand-in Maplibre map instance handed to `onReady`/`onLoad`/`onMoveEnd`
/// callbacks. Bounds are hard-coded so the visible-station queries built from
/// them stay deterministic across tests.
export function createFakeMaplibreMap() {
  return {
    getBounds: () => ({
      getSouth: () => 51,
      getWest: () => 7,
      getNorth: () => 52,
      getEast: () => 8,
    }),
    getZoom: () => 13,
    easeTo: vi.fn(),
    fitBounds: vi.fn(),
    on: vi.fn(),
    off: vi.fn(),
    remove: vi.fn(),
  }
}

/// Map mock: renders children in a `div` (so Marker/Popup trees are real DOM)
/// and fires `onLoad` + `onMoveEnd` after mount so the consumer's viewport
/// state (bounds/zoom) initialises and the markers/clusters render. The
/// received props are also stashed on `window.__maplibreProps` for assertions.
function MaplibreMapMock(props: {
  children?: ReactNode
  onLoad?: (event: { target: unknown }) => void
  onMoveEnd?: (event: { target: unknown }) => void
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [key: string]: any
}) {
  const { children, onLoad, onMoveEnd, ...rest } = props
  useEffect(() => {
    const map = createFakeMaplibreMap()
    onLoad?.({ target: map })
    onMoveEnd?.({ target: map })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return (
    <div data-testid="maplibre-Map" data-maplibre-props={JSON.stringify(rest)}>
      {children}
    </div>
  )
}

function MaplibreMarkerMock(props: { children?: ReactNode; [key: string]: unknown }) {
  const { children, ...rest } = props
  return (
    <div
      data-testid="maplibre-Marker"
      data-maplibre-marker-props={JSON.stringify(rest)}
      className="maplibregl-marker"
    >
      {children}
    </div>
  )
}

function MaplibrePopupMock(props: { children?: ReactNode; [key: string]: unknown }) {
  const { children, ...rest } = props
  return (
    <div
      data-testid="maplibre-Popup"
      data-maplibre-popup-props={JSON.stringify(rest)}
      className="maplibregl-popup"
    >
      {children}
    </div>
  )
}

// ---- Library mocks --------------------------------------------------------

vi.mock('@vis.gl/react-maplibre', () => ({
  Map: MaplibreMapMock,
  Marker: MaplibreMarkerMock,
  Popup: MaplibrePopupMock,
  NavigationControl: () => <div data-testid="maplibre-NavigationControl" />,
  ScaleControl: () => <div data-testid="maplibre-ScaleControl" />,
  useMap: () => ({ current: null }),
  useControl: () => null,
}))

// maplibre-gl: value imports used at module scope in lib/map.tsx plus the
// class types consumers hold. No real WebGL map is created in unit tests.
vi.mock('maplibre-gl', () => {
  const FakeMap = function FakeMap() {}
  FakeMap.prototype.getBounds = () => null
  return {
    Map: FakeMap,
    Marker: class {},
    Popup: class {},
    NavigationControl: class {},
    addProtocol: vi.fn(),
    setWorkerUrl: vi.fn(),
  }
})

// recharts: no-op components that render their children, so the chart
// wrappers' surrounding logic (data prep, empty/limit states, tooltips)
// executes without a canvas/SVG measurement environment.
function rechartsEl(name: string) {
  return function RechartsMock(props: { children?: ReactNode; [key: string]: unknown }) {
    const { children, ...rest } = props
    return (
      <div data-testid={`recharts-${name}`} data-recharts-props={JSON.stringify(rest)}>
        {children}
      </div>
    )
  }
}

vi.mock('recharts', () => {
  const chart = (name: string) => rechartsEl(name)
  return {
    ResponsiveContainer: chart('ResponsiveContainer'),
    BarChart: chart('BarChart'),
    Bar: chart('Bar'),
    LineChart: chart('LineChart'),
    Line: chart('Line'),
    AreaChart: chart('AreaChart'),
    Area: chart('Area'),
    PieChart: chart('PieChart'),
    Pie: chart('Pie'),
    Cell: chart('Cell'),
    RadarChart: chart('RadarChart'),
    Radar: chart('Radar'),
    PolarGrid: chart('PolarGrid'),
    PolarAngleAxis: chart('PolarAngleAxis'),
    PolarRadiusAxis: chart('PolarRadiusAxis'),
    XAxis: chart('XAxis'),
    YAxis: chart('YAxis'),
    CartesianGrid: chart('CartesianGrid'),
    Tooltip: chart('Tooltip'),
    Legend: chart('Legend'),
    Label: chart('Label'),
    ReferenceLine: chart('ReferenceLine'),
  }
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})
