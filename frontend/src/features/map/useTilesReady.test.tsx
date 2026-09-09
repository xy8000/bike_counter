import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TILES_URL, useTilesReady } from './useTilesReady'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useTilesReady', () => {
  it('reports ready once the basemap archive is served', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)

    const { result, unmount } = renderHook(() => useTilesReady())
    await waitFor(() => expect(result.current).toBe(true))
    unmount()

    expect(fetchMock).toHaveBeenCalledWith(TILES_URL, { method: 'HEAD', cache: 'no-store' })
  })

  it('stays not ready while the archive returns 404 (build still in progress)', () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 404 }))
    vi.stubGlobal('fetch', fetchMock)

    const { result, unmount } = renderHook(() => useTilesReady())
    // The probe answered but the archive is not served yet: still not ready.
    expect(result.current).toBe(false)
    expect(fetchMock).toHaveBeenCalledWith(TILES_URL, { method: 'HEAD', cache: 'no-store' })
    unmount()
  })
})
