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
