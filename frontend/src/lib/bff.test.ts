import { ok } from '@/test-utils/http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { getJson, unwrapLinks } from './bff'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('getJson', () => {
  it('parses the JSON body of an allowlisted BFF route', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok({ value: 42 }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson<{ value: number }>('/api/bff/version')).resolves.toEqual({ value: 42 })
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/version')
  })

  it('keeps a legitimate query string untouched', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok([]))
    vi.stubGlobal('fetch', fetchMock)

    const url = '/api/bff/station-detail/s1/graphs/week?as_of=2026-01-01T00:00:00Z&resolution=hour'
    await getJson(url)
    expect(fetchMock).toHaveBeenCalledWith(url)
  })

  it('reports the caller URL when the response is not ok', async () => {
    const fetchMock = vi.fn().mockResolvedValue(ok({ error: 'nope' }, 500))
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('/api/bff/data-sources/ms/imports?offset=0&limit=50')).rejects.toThrow(
      '/api/bff/data-sources/ms/imports?offset=0&limit=50 responded with 500',
    )
  })

  it('rejects an off-origin URL before fetching (CSRF/SSRF guard)', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('https://evil.example/steal')).rejects.toThrow(
      'Refusing to fetch non-BFF URL: https://evil.example/steal',
    )
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a protocol-relative URL before fetching', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('//evil.example/steal')).rejects.toThrow('Refusing to fetch non-BFF URL')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a percent-encoded traversal before fetching', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('/api/bff/station-detail/..%2f..%2fadmin')).rejects.toThrow(
      'Refusing to fetch non-BFF URL',
    )
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a same-origin path outside the BFF namespace', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('/api/v1/measurements')).rejects.toThrow('Refusing to fetch non-BFF URL')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a BFF path that is not on the route allowlist', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('/api/bff/admin/reset')).rejects.toThrow(
      'Refusing to fetch non-allowlisted BFF URL: /api/bff/admin/reset',
    )
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects an unknown query parameter before fetching', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getJson('/api/bff/version?url=https://evil.example')).rejects.toThrow(
      'Refusing to fetch non-BFF URL',
    )
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a body delivered through a cross-origin redirect', async () => {
    const redirected = {
      ok: true,
      status: 200,
      url: 'https://evil.example/steal',
      json: async () => ({ value: 1 }),
    } as unknown as Response
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(redirected))

    await expect(getJson('/api/bff/version')).rejects.toThrow(
      'Refusing to read a cross-origin response: /api/bff/version',
    )
  })

  it('accepts a same-origin final response URL', async () => {
    const origin = globalThis.location.origin
    const response = {
      ok: true,
      status: 200,
      url: `${origin}/api/bff/version`,
      json: async () => ({ version: '0.0.11' }),
    } as unknown as Response
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response))

    await expect(getJson('/api/bff/version')).resolves.toEqual({ version: '0.0.11' })
  })
})

describe('unwrapLinks', () => {
  it('reduces LinkDto values to their href', () => {
    expect(
      unwrapLinks<{ self: string; next: string }>({
        self: { href: '/api/bff/stations' },
        next: { href: '/api/bff/stations?page=2', templated: true },
      }),
    ).toEqual({ self: '/api/bff/stations', next: '/api/bff/stations?page=2' })
  })
})
