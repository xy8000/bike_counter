import { ok } from '@/test-utils/http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { getJson, unwrapLinks } from './bff'

/// Stubs `fetch` with a responder and returns the spy, so every test in this
/// suite asserts the same (single-argument) call shape and a rejected URL can be
/// proven to never reach the network.
function stubFetch(respond: () => Response | Promise<Response> = () => ok({})) {
  const fetchMock = vi.fn(() => Promise.resolve(respond()))
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

/// A `Response` look-alike with an explicit final URL, i.e. what `fetch` exposes
/// after following redirects (a real, non-redirected `Response` has `url: ''`).
function responseWithUrl(url: string, body: unknown): Response {
  return { ok: true, status: 200, url, json: async () => body } as unknown as Response
}

/// Every payload the guard must refuse, with the message it must throw. One
/// table instead of one near-identical test per payload (keeps the suite free of
/// copy-paste duplication).
const REJECTED_URLS = [
  [
    'an off-origin URL',
    'https://evil.example/steal',
    'Refusing to fetch non-BFF URL: https://evil.example/steal',
  ],
  ['a protocol-relative URL', '//evil.example/steal', 'Refusing to fetch non-BFF URL'],
  [
    'a percent-encoded traversal',
    '/api/bff/station-detail/..%2f..%2fadmin',
    'Refusing to fetch non-BFF URL',
  ],
  [
    'a same-origin path outside the BFF namespace',
    '/api/v1/measurements',
    'Refusing to fetch non-BFF URL',
  ],
  [
    'a BFF path that is not on the route allowlist',
    '/api/bff/admin/reset',
    'Refusing to fetch non-allowlisted BFF URL: /api/bff/admin/reset',
  ],
  [
    'an unknown query parameter',
    '/api/bff/version?url=https://evil.example',
    'Refusing to fetch non-BFF URL',
  ],
] as const

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('getJson', () => {
  it('parses the JSON body of an allowlisted BFF route', async () => {
    const fetchMock = stubFetch(() => ok({ value: 42 }))

    await expect(getJson<{ value: number }>('/api/bff/version')).resolves.toEqual({ value: 42 })
    expect(fetchMock).toHaveBeenCalledWith('/api/bff/version')
  })

  it('keeps a legitimate query string untouched', async () => {
    const fetchMock = stubFetch(() => ok([]))

    const url = '/api/bff/station-detail/s1/graphs/week?as_of=2026-01-01T00:00:00Z&resolution=hour'
    await getJson(url)
    expect(fetchMock).toHaveBeenCalledWith(url)
  })

  it('reports the caller URL when the response is not ok', async () => {
    stubFetch(() => ok({ error: 'nope' }, 500))

    const url = '/api/bff/data-sources/ms/imports?offset=0&limit=50'
    await expect(getJson(url)).rejects.toThrow(`${url} responded with 500`)
  })

  it.each(REJECTED_URLS)('rejects %s before fetching', async (_case, url, message) => {
    const fetchMock = stubFetch()

    await expect(getJson(url)).rejects.toThrow(message)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a body delivered through a cross-origin redirect', async () => {
    stubFetch(() => responseWithUrl('https://evil.example/steal', { value: 1 }))

    await expect(getJson('/api/bff/version')).rejects.toThrow(
      'Refusing to read a cross-origin response: /api/bff/version',
    )
  })

  it('accepts a same-origin final response URL', async () => {
    stubFetch(() =>
      responseWithUrl(`${globalThis.location.origin}/api/bff/version`, { version: '0.0.11' }),
    )

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
