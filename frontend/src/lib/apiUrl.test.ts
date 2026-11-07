import { describe, expect, it } from 'vitest'
import { assertSafeBffUrl, assertSafeRequestUrl, safePathSegment } from './apiUrl'

describe('assertSafeRequestUrl', () => {
  it('returns a root-relative same-origin path unchanged', () => {
    expect(assertSafeRequestUrl('/api/bff/stations?min_lat=51')).toBe(
      '/api/bff/stations?min_lat=51',
    )
    expect(assertSafeRequestUrl('/api/overview/s1')).toBe('/api/overview/s1')
  })

  it('rejects absolute cross-origin URLs', () => {
    expect(() => assertSafeRequestUrl('https://evil.example/steal')).toThrow(
      'Refusing to fetch unsafe URL: https://evil.example/steal',
    )
  })

  it('rejects protocol-relative URLs', () => {
    expect(() => assertSafeRequestUrl('//evil.example/steal')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })

  it('rejects backslash-smuggled paths', () => {
    expect(() => assertSafeRequestUrl('/\\evil.example/steal')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })

  it('rejects control characters', () => {
    expect(() => assertSafeRequestUrl('/api/bff/x\nHost: evil')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })

  it('rejects fragments', () => {
    expect(() => assertSafeRequestUrl('/api/bff/version#steal')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })

  it('rejects whitespace anywhere in the URL', () => {
    expect(() => assertSafeRequestUrl('/api/bff/version ')).toThrow('Refusing to fetch unsafe URL')
  })

  it('normalizes resolved dot segments and rejects the resulting route', () => {
    expect(() => assertSafeRequestUrl('/api/bff/version/../secret')).toThrow(
      'Refusing to fetch unsafe URL: /api/bff/version/../secret',
    )
  })

  it('rejects empty path segments', () => {
    expect(() => assertSafeRequestUrl('/api//bff/version')).toThrow('Refusing to fetch unsafe URL')
  })

  it('rejects unknown query parameters', () => {
    expect(() => assertSafeRequestUrl('/api/bff/version?redirect=https://evil.example')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })

  it('rejects malformed percent escapes in the query', () => {
    expect(() => assertSafeRequestUrl('/api/bff/version?as_of=%ZZ')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })

  it('rejects percent-encoded control characters in the query', () => {
    expect(() => assertSafeRequestUrl('/api/bff/version?as_of=%0d%0aHost:%20evil')).toThrow(
      'Refusing to fetch unsafe URL',
    )
  })
})

describe('assertSafeBffUrl', () => {
  it('returns a root-relative BFF path unchanged', () => {
    expect(assertSafeBffUrl('/api/bff/stations?min_lat=51')).toBe('/api/bff/stations?min_lat=51')
  })

  it('rejects absolute cross-origin URLs', () => {
    expect(() => assertSafeBffUrl('https://evil.example/steal')).toThrow(
      'Refusing to fetch non-BFF URL: https://evil.example/steal',
    )
  })

  it('rejects protocol-relative URLs', () => {
    expect(() => assertSafeBffUrl('//evil.example/steal')).toThrow('Refusing to fetch non-BFF URL')
  })

  it('rejects backslash-smuggled paths', () => {
    expect(() => assertSafeBffUrl('/api/bff/..\\..\\evil')).toThrow('Refusing to fetch non-BFF URL')
  })

  it('rejects control characters', () => {
    expect(() => assertSafeBffUrl('/api/bff/x\nHost: evil')).toThrow(
      'Refusing to fetch non-BFF URL',
    )
  })

  it('rejects same-origin paths outside the BFF prefix', () => {
    expect(() => assertSafeBffUrl('/api/v1/measurements')).toThrow('Refusing to fetch non-BFF URL')
  })

  it('rejects fragments', () => {
    expect(() => assertSafeBffUrl('/api/bff/version#steal')).toThrow(
      'Refusing to fetch non-BFF URL',
    )
  })

  it('rejects userinfo smuggling', () => {
    expect(() => assertSafeBffUrl('/api/bff/@evil.example/version')).toThrow(
      'Refusing to fetch non-BFF URL',
    )
  })

  it('rejects percent-encoded traversal segments', () => {
    expect(() => assertSafeBffUrl('/api/bff/station-detail/%2e%2e%2fadmin')).toThrow(
      'Refusing to fetch non-BFF URL',
    )
  })

  it('rejects unknown query parameters', () => {
    expect(() => assertSafeBffUrl('/api/bff/version?url=https://evil.example')).toThrow(
      'Refusing to fetch non-BFF URL',
    )
  })

  it('rejects BFF paths that are not on the route allowlist', () => {
    expect(() => assertSafeBffUrl('/api/bff/admin/reset')).toThrow(
      'Refusing to fetch non-allowlisted BFF URL: /api/bff/admin/reset',
    )
  })

  it('accepts every allowlisted BFF route shape', () => {
    const routes = [
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
      '/api/bff/station-overview/2b410a8a-3474-4ab3-9fa4-d88b92256bee',
      '/api/bff/station-overview/2b410a8a-3474-4ab3-9fa4-d88b92256bee/stats',
      '/api/bff/station-detail/2b410a8a-3474-4ab3-9fa4-d88b92256bee',
      '/api/bff/station-detail/2b410a8a-3474-4ab3-9fa4-d88b92256bee/overview',
      '/api/bff/station-detail/2b410a8a-3474-4ab3-9fa4-d88b92256bee/graphs/last_30_days',
      '/api/bff/station-detail/2b410a8a-3474-4ab3-9fa4-d88b92256bee/monthly',
      '/api/bff/assets/2b410a8a-3474-4ab3-9fa4-d88b92256bee/content',
      '/api/bff/data-sources',
      '/api/bff/data-sources/2b410a8a-3474-4ab3-9fa4-d88b92256bee',
      '/api/bff/data-sources/2b410a8a-3474-4ab3-9fa4-d88b92256bee/imports?offset=0&limit=50',
    ]
    for (const route of routes) {
      expect(assertSafeBffUrl(route)).toBe(route)
    }
  })
})

describe('safePathSegment', () => {
  it('returns a plain id or slug unchanged', () => {
    expect(safePathSegment('2b410a8a-3474-4ab3-9fa4-d88b92256bee')).toBe(
      '2b410a8a-3474-4ab3-9fa4-d88b92256bee',
    )
    expect(safePathSegment('muenster-1')).toBe('muenster-1')
  })

  it('rejects values that could change the request path or query', () => {
    for (const unsafe of ['../admin', 'a/b', 'a?b=1', 'a#frag', 'a%2fb', 'a&b', '.', '..', '']) {
      expect(() => safePathSegment(unsafe)).toThrow('Refusing to use unsafe path segment')
    }
  })
})
