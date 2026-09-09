import { describe, expect, it } from 'vitest'
import {
  escapeHtml,
  formatFullDate,
  formatFullDateTime,
  formatNumber,
  formatTimestamp,
} from './format'

describe('formatNumber', () => {
  it('formats with the de-DE grouping separator', () => {
    expect(formatNumber(0)).toBe('0')
    expect(formatNumber(1234)).toBe('1.234')
    expect(formatNumber(1000000)).toBe('1.000.000')
  })
})

describe('formatTimestamp', () => {
  it('returns "never" for null, empty and unparseable values', () => {
    expect(formatTimestamp(null)).toBe('never')
    expect(formatTimestamp('')).toBe('never')
    expect(formatTimestamp('not-a-date')).toBe('never')
  })

  it('formats a valid ISO timestamp as a short de-DE date-time', () => {
    // de-DE short date uses a two-digit year ("15.06.24, 14:00"); a regex keeps
    // the assertion independent of the runner's time zone.
    expect(formatTimestamp('2024-06-15T12:00:00Z')).toMatch(/^\d{2}\.\d{2}\.\d{2}, \d{2}:\d{2}$/)
  })
})

describe('formatFullDate', () => {
  it('formats a local date as dd.MM.yyyy', () => {
    expect(formatFullDate(new Date(1998, 11, 31).getTime())).toBe('31.12.1998')
  })
})

describe('formatFullDateTime', () => {
  it('formats a local date-time with date and time components', () => {
    expect(formatFullDateTime(new Date(1998, 11, 31, 14, 5).getTime())).toMatch(/31\.12\.1998/)
  })
})

describe('escapeHtml', () => {
  // Compare numeric char codes so the assertion never stores (or is distorted
  // by) an HTML-entity sequence in source or output.
  const codes = (value: string): number[] => [...value].map((char) => char.charCodeAt(0))

  it('escapes the five HTML-significant characters', () => {
    expect(codes(escapeHtml('<'))).toEqual([38, 108, 116, 59]) // "<"
    expect(codes(escapeHtml('>'))).toEqual([38, 103, 116, 59]) // ">"
    expect(codes(escapeHtml('&'))).toEqual([38, 97, 109, 112, 59]) // "&"
    expect(codes(escapeHtml('"'))).toEqual([38, 113, 117, 111, 116, 59]) // """
    expect(codes(escapeHtml("'"))).toEqual([38, 35, 51, 57, 59]) // "'"
  })

  it('leaves strings without special characters untouched', () => {
    expect(escapeHtml('plain text')).toBe('plain text')
  })
})
