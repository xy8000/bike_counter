import { ok } from '@/test-utils/http'
import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useVersion } from './useVersion'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useVersion', () => {
  it('loads the backend version on mount', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok({ version: '0.0.8' }))
    vi.stubGlobal('fetch', fetchMock)

    const { result } = renderHook(() => useVersion())

    expect(result.current.backendVersion).toBeNull()
    expect(result.current.error).toBe(false)

    await waitFor(() => expect(result.current.backendVersion).toBe('0.0.8'))
    expect(result.current.error).toBe(false)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/version')
  })

  it('reports the error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network down')))

    const { result } = renderHook(() => useVersion())

    await waitFor(() => expect(result.current.error).toBe(true))
    expect(result.current.backendVersion).toBeNull()
  })
})
