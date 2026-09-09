import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useResource } from './useResource'

function ok(data: unknown) {
  return new Response(JSON.stringify(data), { status: 200 })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useResource', () => {
  it('loads the data and clears the loading flag on success', async () => {
    const payload = { value: 42 }
    const fetchMock = vi.fn().mockResolvedValue(ok(payload))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useResource<{ value: number }>('/api/card'))

    expect(result.current.loading).toBe(true)
    expect(result.current.data).toBeNull()
    expect(result.current.error).toBe(false)

    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.data).toEqual(payload)
    expect(result.current.error).toBe(false)
    expect(fetchMock).toHaveBeenCalledWith('/api/card')
  })

  it('does not fetch and stays idle when url is null', () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useResource<string | null>(null))

    expect(result.current.loading).toBe(false)
    expect(result.current.data).toBeNull()
    expect(result.current.error).toBe(false)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('refetches when the url changes', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ok({ value: 1 }))
      .mockResolvedValueOnce(ok({ value: 2 }))
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ url }: { url: string | null }) => useResource<{ value: number }>(url),
      { initialProps: { url: '/api/a' } },
    )
    await waitFor(() => expect(result.current.data).toEqual({ value: 1 }))

    rerender({ url: '/api/b' })
    expect(result.current.loading).toBe(true)
    expect(result.current.data).toBeNull()

    await waitFor(() => expect(result.current.data).toEqual({ value: 2 }))
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock).toHaveBeenLastCalledWith('/api/b')
  })

  it('reports the error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

    const { result } = renderHook(() => useResource<unknown>('/api/card'))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
    expect(result.current.data).toBeNull()
  })

  it('reports the error when the response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

    const { result } = renderHook(() => useResource<unknown>('/api/card'))

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.loading).toBe(false)
  })

  it('ignores the stale response when the url changes mid-flight', async () => {
    let resolveA!: (response: Response) => void
    let resolveB!: (response: Response) => void
    const fetchMock = vi.fn((url: string) => {
      if (url === '/api/a') return new Promise<Response>((resolve) => (resolveA = resolve))
      return new Promise<Response>((resolve) => (resolveB = resolve))
    })
    vi.stubGlobal('fetch', fetchMock)

    const { result, rerender } = renderHook(
      ({ url }: { url: string | null }) => useResource<{ value: number }>(url),
      { initialProps: { url: '/api/a' } },
    )

    rerender({ url: '/api/b' })
    act(() => {
      resolveA(ok({ value: 1 }))
      resolveB(ok({ value: 2 }))
    })

    await waitFor(() => expect(result.current.data).toEqual({ value: 2 }))
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('ignores the resolution after the hook unmounts', async () => {
    let resolvePending!: (response: Response) => void
    const fetchMock = vi.fn(() => new Promise<Response>((resolve) => (resolvePending = resolve)))
    vi.stubGlobal('fetch', fetchMock)

    const { unmount } = renderHook(() => useResource<{ value: number }>('/api/card'))

    unmount()
    act(() => {
      resolvePending(ok({ value: 1 }))
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})
