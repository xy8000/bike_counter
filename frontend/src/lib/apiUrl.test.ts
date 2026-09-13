import { describe, expect, it } from 'vitest'
import { assertSafeBffUrl } from './apiUrl'

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
})
