import { render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Bounds } from '../../lib/geo'
import { BaseMap } from './BaseMap'

const BOUNDS: Bounds = { min_lat: 51, min_lng: 7, max_lat: 52, max_lng: 8 }

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

/// The committed style: a vector source whose URL carries the runtime
/// placeholder plus a non-vector source that must be left untouched.
function stylePayload() {
  return {
    version: 8,
    sources: {
      protomaps: {
        type: 'vector',
        url: 'pmtiles://REPLACED_AT_RUNTIME/tiles/map.pmtiles',
      },
      // A non-vector source with its own url must be left untouched.
      raster: { type: 'raster', url: 'https://example.invalid/tiles/{z}/{x}/{y}.png' },
    },
  }
}

function mapElement(): HTMLElement {
  const el = document.querySelector('[data-testid="maplibre-Map"]')
  if (!el) throw new Error('maplibre Map not rendered')
  return el as HTMLElement
}

function mapProps(): Record<string, unknown> {
  return JSON.parse(mapElement().getAttribute('data-maplibre-props') ?? '{}')
}

/// A matchMedia stub that lets the test drive the colour-scheme `change`
/// listeners the basemap hook registers.
function stubMatchMedia(initialMatches: boolean) {
  const listeners: Array<(event: { matches: boolean }) => void> = []
  const mql = {
    matches: initialMatches,
    media: '',
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn((_type: string, cb: (event: { matches: boolean }) => void) => {
      listeners.push(cb)
    }),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }
  const impl = vi.fn(() => mql)
  ;(window.matchMedia as unknown as ReturnType<typeof vi.fn>).mockImplementation(impl as never)
  return { mql, listeners }
}

function renderBaseMap(overrides: {
  bounds?: Bounds
  interactive?: boolean
  scrollZoom?: boolean
  navigationControl?: boolean
  children?: ReactNode
}) {
  const onReady = vi.fn()
  const onBounds = vi.fn()
  const onZoom = vi.fn()
  const onVoidClick = vi.fn()
  const utils = render(
    <BaseMap
      bounds={overrides.bounds}
      interactive={overrides.interactive}
      scrollZoom={overrides.scrollZoom}
      navigationControl={overrides.navigationControl}
      onReady={onReady}
      onBounds={onBounds}
      onZoom={onZoom}
      onVoidClick={onVoidClick}
    >
      {overrides.children ?? <div data-testid="map-child" />}
    </BaseMap>,
  )
  return { ...utils, onReady, onBounds, onZoom, onVoidClick }
}

afterEach(() => {
  vi.unstubAllGlobals()
  // Restore the shared matchMedia default (matches:false) so later tests in
  // this file (and other files) start from the baseline again.
  ;(window.matchMedia as unknown as ReturnType<typeof vi.fn>).mockImplementation(
    (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  )
})

describe('BaseMap', () => {
  it('renders nothing while the basemap style is still being fetched', () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise<Response>(() => {})),
    )

    const { container } = renderBaseMap({})

    expect(document.querySelector('[data-testid="maplibre-Map"]')).toBeNull()
    expect(container).toBeEmptyDOMElement()
  })

  it('renders the Map once the style loads, replacing the runtime origin in vector sources', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(stylePayload())))
    const { onReady, onBounds, onZoom } = renderBaseMap({ navigationControl: true })

    const map = await screen.findByTestId('maplibre-Map')
    expect(map).toBeInTheDocument()

    const props = mapProps()
    const sources = (props.mapStyle as { sources: Record<string, { url?: string }> }).sources
    expect(sources.protomaps.url).toBe(`pmtiles://${window.location.origin}/tiles/map.pmtiles`)
    // Non-vector sources are left untouched by the placeholder replacement.
    expect(sources.raster.url).toBe('https://example.invalid/tiles/{z}/{x}/{y}.png')
    expect(JSON.stringify(props.mapStyle)).not.toContain('REPLACED_AT_RUNTIME')

    // Children + navigation control are forwarded into the mocked Map.
    expect(screen.getByTestId('map-child')).toBeInTheDocument()
    expect(screen.getByTestId('maplibre-NavigationControl')).toBeInTheDocument()

    // The mocked Map fires onLoad + onMoveEnd after mount: the callbacks report
    // the fake map's bounds/zoom to the consumer.
    await waitFor(() => {
      expect(onReady).toHaveBeenCalledTimes(1)
    })
    expect(onBounds).toHaveBeenCalledWith(BOUNDS)
    expect(onZoom).toHaveBeenCalledWith(13)
  })

  it('omits the navigation control when navigationControl is false', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(stylePayload())))

    renderBaseMap({ navigationControl: false })

    await screen.findByTestId('maplibre-Map')
    expect(screen.queryByTestId('maplibre-NavigationControl')).not.toBeInTheDocument()
  })

  it('passes the interaction flags through to the Map', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(stylePayload())))

    renderBaseMap({ interactive: false, scrollZoom: false })

    await screen.findByTestId('maplibre-Map')
    const props = mapProps()
    expect(props.keyboard).toBe(false)
    expect(props.dragPan).toBe(false)
    expect(props.doubleClickZoom).toBe(false)
    expect(props.touchZoomRotate).toBe(false)
    expect(props.scrollZoom).toBe(false)
  })

  it('fits the map to the given bounds on load', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(ok(stylePayload())))
    const { onReady } = renderBaseMap({ bounds: BOUNDS })

    await screen.findByTestId('maplibre-Map')
    await waitFor(() => {
      expect(onReady).toHaveBeenCalled()
    })

    const map = onReady.mock.calls[0][0] as { fitBounds: ReturnType<typeof vi.fn> }
    expect(map.fitBounds).toHaveBeenCalledWith(
      [
        [7, 51],
        [8, 52],
      ],
      { padding: 0, duration: 0 },
    )
  })

  it('falls back to the style URL and logs when the style fetch rejects', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

    renderBaseMap({})

    const map = await screen.findByTestId('maplibre-Map')
    expect(map).toBeInTheDocument()
    expect(mapProps().mapStyle).toBe('/styles/basemap.json')
    expect(errorSpy).toHaveBeenCalledWith(
      'failed to load the self-hosted map style',
      expect.any(Error),
    )
  })

  it('uses the dark basemap when the OS colour scheme is dark', async () => {
    stubMatchMedia(true)
    const fetchMock = vi.fn().mockResolvedValue(ok(stylePayload()))
    vi.stubGlobal('fetch', fetchMock)

    renderBaseMap({})

    await screen.findByTestId('maplibre-Map')
    expect(fetchMock).toHaveBeenCalledWith('/styles/basemap-dark.json')
  })

  it('swaps the basemap when the OS colour scheme changes after mount', async () => {
    const { listeners } = stubMatchMedia(false)
    const fetchMock = vi.fn().mockResolvedValue(ok(stylePayload()))
    vi.stubGlobal('fetch', fetchMock)

    renderBaseMap({})
    await screen.findByTestId('maplibre-Map')
    expect(fetchMock).toHaveBeenCalledWith('/styles/basemap.json')

    // Flip the colour scheme; the hook's change listener re-renders with the
    // dark style and refetches it.
    listeners.forEach((cb) => cb({ matches: true }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/styles/basemap-dark.json')
    })
  })
})
