import { describe, expect, it } from 'vitest'
import { assertSafeBffUrl, assertSafeRequestUrl, safePathSegment } from './apiUrl'

const STATION_ID = '2b410a8a-3474-4ab3-9fa4-d88b92256bee'

/// Payloads both guards must refuse: they leave the origin, break out of the URL
/// context or ask the parser to rewrite the path. One table shared by the two
/// guards instead of one near-identical test per payload.
const UNSAFE_URLS = [
  ['an absolute cross-origin URL', 'https://evil.example/steal'],
  ['a protocol-relative URL', '//evil.example/steal'],
  ['a backslash-smuggled path', '/\\evil.example/steal'],
  ['a control character', '/api/bff/x\nHost: evil'],
  ['a fragment', '/api/bff/version#steal'],
  ['a trailing space', '/api/bff/version '],
  ['a rewritten dot segment', '/api/bff/version/../secret'],
  ['an empty path segment', '/api//bff/version'],
  ['a userinfo segment', '/api/bff/@evil.example/version'],
  ['a percent-encoded traversal', '/api/bff/station-detail/%2e%2e%2fadmin'],
  ['an unknown query parameter', '/api/bff/version?redirect=https://evil.example'],
  ['a malformed percent escape', '/api/bff/version?as_of=%ZZ'],
  ['a percent-encoded control character', '/api/bff/version?as_of=%0d%0aHost:%20evil'],
] as const

/// Structurally safe, same-origin paths that only the BFF guard refuses.
const NON_BFF_URLS = ['/api/v1/measurements', '/api/overview/s1'] as const

/// Every route the backend BFF serves, i.e. exactly what the allowlist admits.
const ALLOWED_BFF_URLS = [
  '/api/bff/stations',
  '/api/bff/stations/sidebar',
  '/api/bff/stations/sidebar/stats',
  '/api/bff/stations/search',
  '/api/bff/stations/summary',
  '/api/bff/stations/summary/overview',
  '/api/bff/stations/summary/graphs/week',
  '/api/bff/stations/summary/monthly',
  '/api/bff/global-summary',
  '/api/bff/version',
  `/api/bff/station-overview/${STATION_ID}`,
  `/api/bff/station-overview/${STATION_ID}/stats`,
  `/api/bff/station-detail/${STATION_ID}`,
  `/api/bff/station-detail/${STATION_ID}/overview`,
  `/api/bff/station-detail/${STATION_ID}/graphs/last_30_days`,
  `/api/bff/station-detail/${STATION_ID}/monthly`,
  `/api/bff/assets/${STATION_ID}/content`,
  '/api/bff/data-sources',
  `/api/bff/data-sources/${STATION_ID}`,
  `/api/bff/data-sources/${STATION_ID}/imports?offset=0&limit=50`,
] as const

describe('assertSafeRequestUrl', () => {
  it.each(['/api/bff/stations?min_lat=51', '/api/overview/s1'])(
    'returns the root-relative same-origin path %s unchanged',
    (url) => {
      expect(assertSafeRequestUrl(url)).toBe(url)
    },
  )

  it.each(UNSAFE_URLS)('rejects %s', (_case, url) => {
    expect(() => assertSafeRequestUrl(url)).toThrow('Refusing to fetch unsafe URL')
  })

  it('names the rejected URL in the error', () => {
    expect(() => assertSafeRequestUrl('https://evil.example/steal')).toThrow(
      'Refusing to fetch unsafe URL: https://evil.example/steal',
    )
  })
})

describe('assertSafeBffUrl', () => {
  it.each(ALLOWED_BFF_URLS)('returns the allowlisted BFF path %s unchanged', (url) => {
    expect(assertSafeBffUrl(url)).toBe(url)
  })

  it.each(UNSAFE_URLS)('rejects %s', (_case, url) => {
    expect(() => assertSafeBffUrl(url)).toThrow('Refusing to fetch non-BFF URL')
  })

  it.each(NON_BFF_URLS)('rejects the non-BFF path %s', (url) => {
    expect(() => assertSafeBffUrl(url)).toThrow('Refusing to fetch non-BFF URL')
  })

  it('names the rejected URL in the error', () => {
    expect(() => assertSafeBffUrl('https://evil.example/steal')).toThrow(
      'Refusing to fetch non-BFF URL: https://evil.example/steal',
    )
  })

  it('names a non-allowlisted BFF route in the error', () => {
    expect(() => assertSafeBffUrl('/api/bff/admin/reset')).toThrow(
      'Refusing to fetch non-allowlisted BFF URL: /api/bff/admin/reset',
    )
  })
})

describe('safePathSegment', () => {
  it.each([STATION_ID, 'muenster-1'])('returns the plain segment %s unchanged', (value) => {
    expect(safePathSegment(value)).toBe(value)
  })

  it.each(['../admin', 'a/b', 'a?b=1', 'a#frag', 'a%2fb', 'a&b', '.', '..', ''])(
    'rejects the unsafe segment %s',
    (value) => {
      expect(() => safePathSegment(value)).toThrow('Refusing to use unsafe path segment')
    },
  )
})
