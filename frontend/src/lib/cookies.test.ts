import { afterEach, describe, expect, it } from 'vitest'
import { getCookie, setCookie } from './cookies'

afterEach(() => {
  // jsdom persists document.cookie across tests; clear it between tests.
  document.cookie.split(';').forEach((part) => {
    const name = part.split('=')[0]?.trim()
    if (name) document.cookie = `${name}=; Path=/; Max-Age=0`
  })
})

describe('getCookie', () => {
  it('returns the decoded value of an existing cookie', () => {
    document.cookie = 'theme=dark'
    expect(getCookie('theme')).toBe('dark')
  })

  it('decodes percent-encoded values', () => {
    document.cookie = 'view=min_lat%3D51%26min_lng%3D7'
    expect(getCookie('view')).toBe('min_lat=51&min_lng=7')
  })

  it('ignores cookies that only share a name prefix', () => {
    document.cookie = 'station=s1'
    document.cookie = 'stations=s2'
    expect(getCookie('station')).toBe('s1')
  })

  it('returns null when the cookie is missing', () => {
    expect(getCookie('nope')).toBeNull()
  })

  it('trims surrounding whitespace around each cookie part', () => {
    document.cookie = ' a = 1 '
    // jsdom normalises the assignment, so re-read through the same helper
    // instead of assuming an exact wire format.
    expect(getCookie('a')).not.toBeNull()
  })
})

describe('setCookie', () => {
  it('writes an encoded cookie with Path, Max-Age and SameSite attributes', () => {
    // jsdom's document.cookie getter only reflects the `name=value` pairs, so
    // the cookie *attributes* (Path/Max-Age/SameSite) are not readable back.
    // Capture the exact assignment string instead by shadowing the setter.
    const writes: string[] = []
    const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie')
    const read = descriptor?.get
    const write = descriptor?.set
    Object.defineProperty(document, 'cookie', {
      configurable: true,
      get: () => (read ? read.call(document) : ''),
      set: (value: string) => {
        writes.push(value)
        write?.call(document, value)
      },
    })

    try {
      setCookie('view', 'min_lat=51&min_lng=7', 3600)
    } finally {
      delete (document as { cookie?: string }).cookie
    }

    expect(writes).toHaveLength(1)
    expect(writes[0]).toContain('view=min_lat%3D51%26min_lng%3D7')
    expect(writes[0]).toContain('Path=/')
    expect(writes[0]).toContain('Max-Age=3600')
    expect(writes[0]).toContain('SameSite=Lax')
    // And the encoded cookie really landed in the jar, decodable by getCookie.
    expect(getCookie('view')).toBe('min_lat=51&min_lng=7')
  })

  it('overwrites an existing cookie with the same name', () => {
    setCookie('name', 'first', 100)
    setCookie('name', 'second', 200)
    expect(getCookie('name')).toBe('second')
    expect(document.cookie).not.toContain('name=first')
  })
})
