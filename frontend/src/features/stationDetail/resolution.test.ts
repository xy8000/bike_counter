import { describe, expect, it } from 'vitest'
import {
  DEFAULT_RESOLUTION,
  isResolutionLevel,
  resolutionGranularity,
  resolutionOptions,
  withResolutionParam,
} from './resolution'

describe('resolutionOptions', () => {
  it('returns the fixed set for a fixed timeframe', () => {
    expect(resolutionOptions('day', null, null).map((option) => option.granularity)).toEqual([
      '15m',
      '30m',
      'hour',
    ])
    expect(resolutionOptions('year', null, null).map((option) => option.granularity)).toEqual([
      'day',
      'week',
      'month',
    ])
  })

  it('falls back to the week set for an incomplete individual range', () => {
    const options = resolutionOptions('individual', null, '2024-01-05')
    expect(options.map((option) => option.level)).toEqual(['high', 'mid', 'low'])
    expect(options).toEqual(resolutionOptions('week', null, null))
  })

  it('picks the day set for a 1–2 day individual range', () => {
    expect(
      resolutionOptions('individual', '2024-01-01', '2024-01-02').map((o) => o.granularity),
    ).toEqual(['15m', '30m', 'hour'])
  })

  it('uses the multi-year set beyond 366 days', () => {
    expect(
      resolutionOptions('individual', '2020-01-01', '2024-12-31').map((o) => o.granularity),
    ).toEqual(['week', 'month', 'quarter'])
  })
})

describe('resolutionGranularity', () => {
  it('returns the granularity of the requested level', () => {
    expect(resolutionGranularity('high', 'week', null, null)).toBe('30m')
    expect(resolutionGranularity('low', 'year', null, null)).toBe('month')
    expect(resolutionGranularity('high', 'individual', null, null)).toBe('30m')
  })
})

describe('isResolutionLevel', () => {
  it('accepts the three levels and rejects everything else', () => {
    expect(isResolutionLevel('high')).toBe(true)
    expect(isResolutionLevel('mid')).toBe(true)
    expect(isResolutionLevel('low')).toBe(true)
    expect(isResolutionLevel('ultra')).toBe(false)
    expect(isResolutionLevel('')).toBe(false)
    expect(isResolutionLevel(null)).toBe(false)
    expect(isResolutionLevel(undefined)).toBe(false)
  })
})

describe('withResolutionParam', () => {
  it('appends the resolution param with the correct separator', () => {
    expect(withResolutionParam('https://x/graphs_day', '30m')).toBe(
      'https://x/graphs_day?resolution=30m',
    )
    expect(withResolutionParam('https://x/graphs_year?a=1', 'day')).toBe(
      'https://x/graphs_year?a=1&resolution=day',
    )
  })
})

describe('DEFAULT_RESOLUTION', () => {
  it('is the middle level', () => {
    expect(DEFAULT_RESOLUTION).toBe('mid')
  })
})
