import { describe, expect, it } from 'vitest'
import type { BarSeries } from './TimeSeriesBarChart'
import {
  TIMEFRAMES,
  alignSeries,
  customTimeframeConfig,
  dateFromInput,
  dateToInput,
  fixedTimeframeConfig,
  granularitySubtitle,
  timeframeSeries,
} from './timeframes'
import type { TimeBucket } from './types'

const bucket = (start: string, total: number): TimeBucket => ({ start, total })
const series = (key: string, data: TimeBucket[]): BarSeries => ({
  key,
  label: key,
  stackId: key,
  data,
})

describe('dateFromInput / dateToInput', () => {
  it('round-trips a YYYY-MM-DD value through a browser-local date', () => {
    expect(dateToInput(dateFromInput('2024-03-05'))).toBe('2024-03-05')
  })

  it('parses only the year when month/day are missing', () => {
    expect(dateFromInput('2024').getFullYear()).toBe(2024)
  })
})

describe('granularitySubtitle', () => {
  it('describes every bucket resolution', () => {
    expect(granularitySubtitle('15m')).toBe('15-minute buckets')
    expect(granularitySubtitle('30m')).toBe('30-minute buckets')
    expect(granularitySubtitle('hour')).toBe('1-hour buckets')
    expect(granularitySubtitle('day')).toBe('1-day buckets')
    expect(granularitySubtitle('week')).toBe('1-week buckets')
    expect(granularitySubtitle('month')).toBe('1-month buckets')
    expect(granularitySubtitle('quarter')).toBe('1-quarter buckets')
  })
})

describe('alignSeries', () => {
  const periodStart = (time: number): number => {
    const date = new Date(time)
    return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  }

  it('returns the series unchanged when there is no anchor', () => {
    const input = [series('current', [bucket('2024-01-01T00:00:00.000Z', 5)])]
    expect(alignSeries(input, Number.NaN, periodStart)).toBe(input)
  })

  it('aligns the series window onto the anchor', () => {
    const input = [series('current', [bucket('2024-01-01T00:00:00.000Z', 5)])]
    const anchor = new Date(2024, 0, 1).getTime()
    const aligned = alignSeries(input, anchor, periodStart)
    expect(aligned[0].data[0].start).toBe('2024-01-01T00:00:00.000Z')
  })
})

describe('timeframeSeries', () => {
  const period = {
    current: [bucket('2024-01-01T00:00:00.000Z', 5)],
    previous: [bucket('2023-12-25T00:00:00.000Z', 2)],
  }

  it('emits only the current series when compare is off', () => {
    expect(timeframeSeries(period, TIMEFRAMES.day, false).map((item) => item.key)).toEqual([
      'current',
    ])
  })

  it('emits current and previous series when compare is on', () => {
    expect(timeframeSeries(period, TIMEFRAMES.day, true).map((item) => item.key)).toEqual([
      'current',
      'previous',
    ])
  })

  it('emits only the previous series when current is empty and compare is on', () => {
    const result = timeframeSeries({ current: [], previous: period.previous }, TIMEFRAMES.day, true)
    expect(result.map((item) => item.key)).toEqual(['previous'])
  })

  it('emits only the current series when compare is on but previous is empty', () => {
    const result = timeframeSeries({ current: period.current, previous: [] }, TIMEFRAMES.day, true)
    expect(result.map((item) => item.key)).toEqual(['current'])
  })
})

describe('fixedTimeframeConfig', () => {
  it('carries the resolution-dependent subtitle', () => {
    expect(fixedTimeframeConfig('day', '15m').subtitle).toBe('15-minute buckets')
    expect(fixedTimeframeConfig('week', 'hour').subtitle).toBe('1-hour buckets')
  })

  it('rotates the long week/hour axis', () => {
    expect(fixedTimeframeConfig('week', 'hour').axisRotate).toBe(true)
    expect(fixedTimeframeConfig('week', 'day').axisRotate).toBe(false)
  })

  it('keeps the timeframe label and key', () => {
    const config = fixedTimeframeConfig('year', 'month')
    expect(config.key).toBe('year')
    expect(config.title).toBe('This year')
  })
})

describe('customTimeframeConfig', () => {
  it('describes a quarter-resolution custom range', () => {
    const config = customTimeframeConfig('quarter', '2020-01-01', '2024-12-31')
    expect(config.key).toBe('individual')
    expect(config.subtitle).toBe('1-quarter buckets')
    expect(config.previousLabel).toBe('')
    expect(config.axis(new Date(2024, 0, 1).getTime())).toMatch(/Q\d/)
  })
})

describe('fixed axes and tooltips', () => {
  const at = new Date(2024, 0, 15, 14, 30).getTime()

  it('labels sub-day and day buckets for each fixed timeframe', () => {
    expect(fixedTimeframeConfig('day', 'hour').axis(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('week', '30m').axis(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('week', 'day').axis(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('year', 'month').axis(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('last_30_days', 'hour').axis(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('last_30_days', 'day').axis(at).length).toBeGreaterThan(0)
  })

  it('collapses consecutive duplicate month labels on the year/day axis', () => {
    const axis = fixedTimeframeConfig('year', 'day').axis
    // Two timestamps in the same month: the second label collapses to ''.
    const first = axis(new Date(2024, 0, 2).getTime())
    const second = axis(new Date(2024, 0, 20).getTime())
    const third = axis(new Date(2024, 1, 1).getTime())
    expect(first.length).toBeGreaterThan(0)
    expect(second).toBe('')
    expect(third.length).toBeGreaterThan(0)
  })

  it('sets the rotate flag for long dense axes only', () => {
    expect(fixedTimeframeConfig('week', '30m').axisRotate).toBe(true)
    expect(fixedTimeframeConfig('week', 'hour').axisRotate).toBe(true)
    expect(fixedTimeframeConfig('week', 'day').axisRotate).toBe(false)
    expect(fixedTimeframeConfig('last_30_days', 'hour').axisRotate).toBe(true)
    expect(fixedTimeframeConfig('last_30_days', 'day').axisRotate).toBe(false)
    expect(fixedTimeframeConfig('year', 'day').axisRotate).toBe(false)
    expect(fixedTimeframeConfig('day', 'hour').axisRotate).toBe(false)
  })

  it('picks the full date-time tooltip for sub-day granularities', () => {
    expect(fixedTimeframeConfig('day', '15m').tooltip(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('last_30_days', '30m').tooltip(at).length).toBeGreaterThan(0)
    expect(fixedTimeframeConfig('last_30_days', 'day').tooltip(at).length).toBeGreaterThan(0)
  })
})

describe('customTimeframeConfig axes', () => {
  it('uses the day+time axis for a multi-day sub-day range and rotates it', () => {
    const config = customTimeframeConfig('hour', '2024-01-01', '2024-01-05')
    const label = config.axis(new Date(2024, 0, 3, 8, 0).getTime())
    expect(label.length).toBeGreaterThan(0)
    expect(config.axisRotate).toBe(true)
  })

  it('keeps a bare time axis for a short sub-day range', () => {
    const config = customTimeframeConfig('30m', '2024-01-01', '2024-01-01')
    expect(config.axis(new Date(2024, 0, 1, 8, 0).getTime())).toContain('8')
    expect(config.axisRotate).toBe(false)
  })

  it('labels month buckets and does not rotate them', () => {
    const config = customTimeframeConfig('month', '2024-01-01', '2025-12-31')
    expect(config.axis(new Date(2024, 5, 1).getTime()).length).toBeGreaterThan(0)
    expect(config.axisRotate).toBe(false)
  })

  it('labels quarter buckets deterministically', () => {
    const config = customTimeframeConfig('quarter', '2020-01-01', '2024-12-31')
    expect(config.axis(new Date(2024, 0, 1).getTime())).toBe('Q1 2024')
  })

  it('uses a year-qualified axis when week/day buckets cross a year', () => {
    const config = customTimeframeConfig('week', '2023-12-01', '2024-01-15')
    const label = config.axis(new Date(2024, 0, 10).getTime())
    expect(label.length).toBeGreaterThan(0)
    expect(config.axisRotate).toBe(true)
  })

  it('uses a plain day axis for same-year week/day buckets and rotates it', () => {
    const config = customTimeframeConfig('day', '2024-01-01', '2024-03-31')
    expect(config.axis(new Date(2024, 2, 15).getTime()).length).toBeGreaterThan(0)
    expect(config.axisRotate).toBe(true)
  })

  it('uses an identity period start so no period shifting happens', () => {
    const config = customTimeframeConfig('hour', '2024-01-01', '2024-01-05')
    const time = new Date(2024, 0, 2).getTime()
    expect(config.periodStart(time)).toBe(time)
  })
})

describe('alignSeries edge cases', () => {
  const periodStart = (time: number): number => {
    const date = new Date(time)
    return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  }

  it('leaves a series with no buckets unchanged', () => {
    const input = [series('current', [])]
    const anchor = new Date(2024, 0, 1).getTime()
    const aligned = alignSeries(input, anchor, periodStart)
    expect(aligned).toEqual(input)
    expect(aligned[0].data).toHaveLength(0)
  })

  it('leaves a series unchanged when its window start is not finite', () => {
    const input = [series('current', [bucket('not-a-date', 5)])]
    const anchor = new Date(2024, 0, 1).getTime()
    const aligned = alignSeries(input, anchor, periodStart)
    expect(aligned[0].data[0].start).toBe('not-a-date')
  })
})

describe('timeframeSeries no-data', () => {
  it('emits no series when both periods are empty', () => {
    expect(timeframeSeries({ current: [], previous: [] }, TIMEFRAMES.day, true)).toEqual([])
    expect(timeframeSeries({ current: [], previous: [] }, TIMEFRAMES.day, false)).toEqual([])
  })
})
