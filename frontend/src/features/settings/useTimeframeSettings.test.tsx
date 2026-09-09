import { act, render, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import { MemoryRouter, useSearchParams } from 'react-router-dom'
import { dateToInput } from '../stationDetail/timeframes'
import {
  customRangeParams,
  useTimeframeSettings,
  withCustomRange,
  type TimeframeSettingsValue,
} from './useTimeframeSettings'

const COOKIE_KEY = 'bike-counter.trends.view'

interface Probe {
  settings: TimeframeSettingsValue
  search: string
}

// The hook is bound to a URL like the pages bind it to `useSearchParams`; the
// probe keeps the current value + serialized query so tests can assert both the
// exposed settings and what actually got written to the URL.
function Harness({ probe }: { probe: { current: Probe | null } }) {
  const [searchParams, setSearchParams] = useSearchParams()
  const settings = useTimeframeSettings(searchParams, setSearchParams)
  probe.current = { settings, search: searchParams.toString() }
  return null
}

function renderTimeframeSettings(initialQuery = '') {
  const probe: { current: Probe | null } = { current: null }
  render(
    <MemoryRouter initialEntries={[initialQuery ? `/?${initialQuery}` : '/']}>
      <Harness probe={probe} />
    </MemoryRouter>,
  )
  const read = () => {
    const current = probe.current
    if (!current) throw new Error('harness probe not initialised')
    return current
  }
  return { read }
}

function setViewCookie(value: Record<string, unknown>) {
  document.cookie = `${COOKIE_KEY}=${encodeURIComponent(JSON.stringify(value))}; Path=/`
}

function paramsOf(search: string) {
  return new URLSearchParams(search)
}

beforeEach(() => {
  localStorage.clear()
  document.cookie = `${COOKIE_KEY}=; Path=/; Max-Age=0`
})

describe('useTimeframeSettings URL parsing', () => {
  it('parses every fixed timeframe from the URL', () => {
    for (const timeframe of ['day', 'week', 'last_30_days', 'year']) {
      const { read } = renderTimeframeSettings(`timeframe=${timeframe}`)
      const settings = read().settings
      expect(settings.timeframe).toBe(timeframe)
      expect(settings.isIndividual).toBe(false)
      expect(settings.from).toBeNull()
      expect(settings.to).toBeNull()
      expect(settings.compare).toBe(false)
      expect(settings.exclude).toBe(false)
      expect(settings.resolution).toBe('mid')
    }
  })

  it('parses the individual timeframe with its range, compare, exclude and resolution', () => {
    const { read } = renderTimeframeSettings(
      'timeframe=individual&from=2024-01-01&to=2024-02-10&compare=1&exclude_new_stations=1&resolution=low',
    )
    const settings = read().settings
    expect(settings.timeframe).toBe('individual')
    expect(settings.isIndividual).toBe(true)
    expect(settings.from).toBe('2024-01-01')
    expect(settings.to).toBe('2024-02-10')
    expect(settings.compare).toBe(true)
    expect(settings.exclude).toBe(true)
    expect(settings.resolution).toBe('low')
  })

  it('normalizes an invalid resolution back to the default', () => {
    const { read } = renderTimeframeSettings('timeframe=week&resolution=ultra')
    expect(read().settings.resolution).toBe('mid')
  })

  it('prefers the URL over the cookie when both are present', () => {
    setViewCookie({ timeframe: 'year', compare: true, exclude: false, resolution: 'high' })
    const { read } = renderTimeframeSettings('timeframe=day')
    expect(read().settings.timeframe).toBe('day')
    expect(read().settings.compare).toBe(false)
    expect(read().settings.resolution).toBe('mid')
  })
})

describe('useTimeframeSettings cookie fallback and defaults', () => {
  it('restores settings from the cookie on a bare URL and writes them back via replace', async () => {
    setViewCookie({ timeframe: 'year', compare: true, exclude: true, resolution: 'high' })
    const { read } = renderTimeframeSettings('')

    const settings = read().settings
    expect(settings.timeframe).toBe('year')
    expect(settings.compare).toBe(true)
    expect(settings.exclude).toBe(true)
    expect(settings.resolution).toBe('high')

    await waitFor(() => {
      const params = paramsOf(read().search)
      expect(params.get('timeframe')).toBe('year')
      expect(params.get('compare')).toBe('1')
      expect(params.get('exclude_new_stations')).toBe('1')
      expect(params.get('resolution')).toBe('high')
    })
    // The cookie is refreshed so the fallback survives the restore.
    expect(document.cookie).toContain(COOKIE_KEY)
    expect(decodeURIComponent(document.cookie)).toContain('"timeframe":"year"')
  })

  it('falls back to the week defaults when the URL is bare and no cookie exists', () => {
    const { read } = renderTimeframeSettings('')
    const settings = read().settings
    expect(settings.timeframe).toBe('week')
    expect(settings.isIndividual).toBe(false)
    expect(settings.from).toBeNull()
    expect(settings.to).toBeNull()
    expect(settings.compare).toBe(false)
    expect(settings.exclude).toBe(false)
    expect(settings.resolution).toBe('mid')
    // Nothing is written to a bare URL for a fresh visitor.
    expect(read().search).toBe('')
  })
})

describe('useTimeframeSettings setters', () => {
  it('switches to individual, filling the default range and keeping compare', () => {
    const { read } = renderTimeframeSettings('timeframe=week&compare=1')
    act(() => read().settings.setTimeframe('individual'))

    const settings = read().settings
    expect(settings.timeframe).toBe('individual')
    expect(settings.isIndividual).toBe(true)
    expect(settings.compare).toBe(true)

    const now = new Date()
    expect(settings.to).toBe(dateToInput(now))
    expect(settings.from).toBe(
      dateToInput(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 90)),
    )
  })

  it('switching back from individual to a fixed timeframe clears from/to', () => {
    const { read } = renderTimeframeSettings('timeframe=individual&from=2024-01-01&to=2024-02-10')
    act(() => read().settings.setTimeframe('week'))

    const settings = read().settings
    expect(settings.timeframe).toBe('week')
    expect(settings.isIndividual).toBe(false)
    expect(settings.from).toBeNull()
    expect(settings.to).toBeNull()
    expect(paramsOf(read().search).get('from')).toBeNull()
    expect(paramsOf(read().search).get('to')).toBeNull()
  })

  it('ignores setFrom on a fixed timeframe', () => {
    const { read } = renderTimeframeSettings('timeframe=week')
    act(() => read().settings.setFrom('2024-05-01'))

    expect(read().settings.from).toBeNull()
    expect(paramsOf(read().search).get('from')).toBeNull()
  })

  it('ignores setTo on a fixed timeframe', () => {
    const { read } = renderTimeframeSettings('timeframe=year')
    act(() => read().settings.setTo('2024-05-01'))

    expect(read().settings.to).toBeNull()
    expect(paramsOf(read().search).get('to')).toBeNull()
  })

  it('updates from on the individual timeframe', () => {
    const { read } = renderTimeframeSettings('timeframe=individual&from=2024-01-01&to=2024-02-10')
    act(() => read().settings.setFrom('2024-01-15'))

    const settings = read().settings
    expect(settings.from).toBe('2024-01-15')
    expect(settings.to).toBe('2024-02-10')
  })

  it('updates to on the individual timeframe', () => {
    const { read } = renderTimeframeSettings('timeframe=individual&from=2024-01-01&to=2024-02-10')
    act(() => read().settings.setTo('2024-03-01'))

    const settings = read().settings
    expect(settings.from).toBe('2024-01-01')
    expect(settings.to).toBe('2024-03-01')
  })

  it('fills the missing range side when the individual range is incomplete', () => {
    const { read } = renderTimeframeSettings('timeframe=individual&from=2024-01-01')
    act(() => read().settings.setFrom('2024-02-01'))
    expect(read().settings.to).toBe(dateToInput(new Date()))

    const second = renderTimeframeSettings('timeframe=individual&to=2024-02-10')
    act(() => second.read().settings.setTo('2024-02-15'))
    const now = new Date()
    expect(second.read().settings.from).toBe(
      dateToInput(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 90)),
    )
  })

  it('toggles the compare flag on and off', () => {
    const { read } = renderTimeframeSettings('timeframe=week')
    act(() => read().settings.setCompare(true))
    expect(read().settings.compare).toBe(true)
    expect(paramsOf(read().search).get('compare')).toBe('1')

    act(() => read().settings.setCompare(false))
    expect(read().settings.compare).toBe(false)
    expect(paramsOf(read().search).get('compare')).toBeNull()
  })

  it('toggles the exclude-new-stations flag on and off', () => {
    const { read } = renderTimeframeSettings('timeframe=week')
    act(() => read().settings.setExclude(true))
    expect(read().settings.exclude).toBe(true)
    expect(paramsOf(read().search).get('exclude_new_stations')).toBe('1')

    act(() => read().settings.setExclude(false))
    expect(read().settings.exclude).toBe(false)
    expect(paramsOf(read().search).get('exclude_new_stations')).toBeNull()
  })

  it('writes a non-default resolution and removes it when back on the default', () => {
    const { read } = renderTimeframeSettings('timeframe=year')
    act(() => read().settings.setResolution('low'))
    expect(read().settings.resolution).toBe('low')
    expect(paramsOf(read().search).get('resolution')).toBe('low')

    act(() => read().settings.setResolution('mid'))
    expect(read().settings.resolution).toBe('mid')
    expect(paramsOf(read().search).get('resolution')).toBeNull()
  })
})

describe('customRangeParams and withCustomRange helpers', () => {
  it('builds from at 00:00 UTC and to at 00:00 UTC of the day after the end', () => {
    const params = customRangeParams('2024-01-01', '2024-03-31')
    expect(params.get('from')).toBe('2024-01-01T00:00:00Z')
    expect(params.get('to')).toBe('2024-04-01T00:00:00Z')
  })

  it('carries the exclusive to across month and year boundaries', () => {
    expect(customRangeParams('2023-12-30', '2023-12-31').get('to')).toBe('2024-01-01T00:00:00Z')
    expect(customRangeParams('2024-02-28', '2024-02-29').get('to')).toBe('2024-03-01T00:00:00Z')
  })

  it('withCustomRange appends the range to a base URL, keeping existing params', () => {
    // URLSearchParams percent-encodes the colons of the timestamps.
    expect(withCustomRange('/graphs?x=1', '2024-01-01', '2024-01-31')).toBe(
      '/graphs?x=1&from=2024-01-01T00%3A00%3A00Z&to=2024-02-01T00%3A00%3A00Z',
    )
    expect(withCustomRange('/graphs', '2024-01-01', '2024-01-31')).toBe(
      '/graphs?from=2024-01-01T00%3A00%3A00Z&to=2024-02-01T00%3A00%3A00Z',
    )
  })
})
