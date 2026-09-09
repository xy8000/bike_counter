import { describe, expect, it } from 'vitest'
import { CHART_PALETTE, MAX_DATA_STREAMS, seriesColor } from './chartUtils'

describe('chart constants', () => {
  it('exposes the five CSS chart tokens', () => {
    expect(CHART_PALETTE).toHaveLength(5)
    expect(CHART_PALETTE[0]).toBe('var(--color-chart-1)')
    expect(CHART_PALETTE[4]).toBe('var(--color-chart-5)')
  })

  it('caps the number of renderable data-streams at 5', () => {
    expect(MAX_DATA_STREAMS).toBe(5)
  })
})

describe('seriesColor', () => {
  it('returns the palette colour of the index', () => {
    expect(seriesColor(0)).toBe(CHART_PALETTE[0])
    expect(seriesColor(2)).toBe(CHART_PALETTE[2])
    expect(seriesColor(4)).toBe(CHART_PALETTE[4])
  })

  it('cycles back to the start beyond the palette length', () => {
    expect(seriesColor(5)).toBe(CHART_PALETTE[0])
    expect(seriesColor(9)).toBe(CHART_PALETTE[4])
    expect(seriesColor(10)).toBe(CHART_PALETTE[0])
  })
})
