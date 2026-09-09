import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import { useGlobalSummary } from './useGlobalSummary'
import { TrendSettingsProvider, useTrendSettings } from '../settings/TrendSettingsContext'
import type { GlobalSummary } from './types'

const SUMMARY: GlobalSummary = {
  station_count: 3,
  channel_count: 6,
  bikes_last_day_total: 90,
  last_update: null,
}

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

// Exposes the provider's setter so a test can flip the Bike-Trends flag and
// trigger the refetch the hook is supposed to react to.
let toggleExclude: ((value: boolean) => void) | null = null

function SettingsProbe() {
  const { setExcludeNewStations } = useTrendSettings()
  toggleExclude = setExcludeNewStations
  return null
}

function wrapper({ children }: { children: ReactNode }) {
  return (
    <TrendSettingsProvider>
      <SettingsProbe />
      {children}
    </TrendSettingsProvider>
  )
}

beforeEach(() => {
  localStorage.clear()
  toggleExclude = null
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useGlobalSummary', () => {
  it('loads the summary on mount', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(SUMMARY))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useGlobalSummary(), { wrapper })

    expect(result.current.summary).toBeNull()
    expect(result.current.error).toBe(false)

    await waitFor(() => expect(result.current.summary).toEqual(SUMMARY))
    expect(result.current.error).toBe(false)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/global-summary')
  })

  it('refetches when the excludeNewStations setting changes', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok(SUMMARY))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useGlobalSummary(), { wrapper })
    await waitFor(() => expect(result.current.summary).toEqual(SUMMARY))

    act(() => toggleExclude?.(true))

    await waitFor(() =>
      expect(fetchMock).toHaveBeenLastCalledWith(
        '/api/bff/global-summary?exclude_new_stations=true',
      ),
    )
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('reports the error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

    const { result } = renderHook(() => useGlobalSummary(), { wrapper })

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.summary).toBeNull()
  })

  it('clears the error again when a later refetch succeeds', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new Error('network down'))
      .mockResolvedValueOnce(ok(SUMMARY))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useGlobalSummary(), { wrapper })
    await waitFor(() => expect(result.current.error).toBe(true))

    act(() => toggleExclude?.(true))

    // Wait for the summary itself: `error` flips to false synchronously when the
    // refetch effect starts, before its fetch has resolved, so asserting on
    // `summary` (which only updates once the second request lands) avoids a race
    // where the check runs while the payload is still in flight.
    await waitFor(() => expect(result.current.summary).toEqual(SUMMARY))
    expect(result.current.error).toBe(false)
  })
})
