import { useCallback, useEffect, useMemo } from 'react'

import { getCookie, setCookie } from '../../lib/cookies'
import {
  DEFAULT_RESOLUTION,
  isResolutionLevel,
  type ResolutionLevel,
} from '../stationDetail/resolution'
import { dateToInput } from '../stationDetail/timeframes'
import type { Timeframe } from '../stationDetail/types'

/// Cookie key for the last-used timeframe/compare/range settings. The URL is
/// the source of truth (sharing restores the exact view); the cookie re-populates
/// a bare URL so the settings are not lost when navigating back.
const COOKIE_KEY = 'bike-counter.trends.view'
const COOKIE_MAX_AGE = 60 * 60 * 24 * 365 // 1 year

const VALID_TIMEFRAMES: Timeframe[] = ['day', 'week', 'last_30_days', 'year', 'individual']

/// The persisted shape stored in the URL and the cookie. `from`/`to` are the
/// user-selected dates (`YYYY-MM-DD`) and only apply to the `individual`
/// timeframe. `compare` is kept even while `individual` is active, so switching
/// back to a fixed interval restores the exact previous value. `resolution` is
/// the Bike-Trends resolution level (`high`/`mid`/`low`), always present
/// (defaults to `mid` for absent/legacy values).
export interface PersistedTimeframeSettings {
  timeframe: Timeframe
  from?: string
  to?: string
  compare: boolean
  /// The Bike-Trends "exclude new stations" flag. Presence of `exclude_new_stations=1`
  /// in the URL means on, so a shared link restores the same filter.
  exclude: boolean
  resolution: ResolutionLevel
}

/// The value consumed by the shared settings dialog and the page header label.
export interface TimeframeSettingsValue {
  timeframe: Timeframe
  from: string | null
  to: string | null
  compare: boolean
  exclude: boolean
  resolution: ResolutionLevel
  isIndividual: boolean
  setTimeframe: (timeframe: Timeframe) => void
  setFrom: (from: string) => void
  setTo: (to: string) => void
  setCompare: (compare: boolean) => void
  setExclude: (exclude: boolean) => void
  setResolution: (resolution: ResolutionLevel) => void
}

/// Normalizes an untrusted/absent resolution value to a valid level.
function normalizeResolution(value: string | null | undefined): ResolutionLevel {
  return isResolutionLevel(value) ? value : DEFAULT_RESOLUTION
}

/// Reads the settings from the URL query params; `null` when no `timeframe`
/// param is present (a bare URL).
function parseUrl(searchParams: URLSearchParams): PersistedTimeframeSettings | null {
  const raw = searchParams.get('timeframe')
  if (!raw || !VALID_TIMEFRAMES.includes(raw as Timeframe)) return null
  const from = searchParams.get('from') ?? undefined
  const to = searchParams.get('to') ?? undefined
  return {
    timeframe: raw as Timeframe,
    from,
    to,
    compare: searchParams.get('compare') === '1',
    exclude: searchParams.get('exclude_new_stations') === '1',
    resolution: normalizeResolution(searchParams.get('resolution')),
  }
}

/// Reads the settings from the cookie; `null` when absent or malformed.
function readCookie(): PersistedTimeframeSettings | null {
  const raw = getCookie(COOKIE_KEY)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Partial<PersistedTimeframeSettings>
    if (parsed.timeframe && VALID_TIMEFRAMES.includes(parsed.timeframe)) {
      return {
        timeframe: parsed.timeframe,
        from: parsed.from,
        to: parsed.to,
        compare: parsed.compare === true,
        exclude: parsed.exclude === true,
        resolution: normalizeResolution(parsed.resolution),
      }
    }
  } catch {
    // Malformed cookie: ignore.
  }
  return null
}

/// Writes the settings into `params` (setting/removing the individual range and
/// the compare flag) without touching any other query params.
function applyToParams(params: URLSearchParams, settings: PersistedTimeframeSettings): void {
  params.set('timeframe', settings.timeframe)
  if (settings.timeframe === 'individual' && settings.from && settings.to) {
    params.set('from', settings.from)
    params.set('to', settings.to)
  } else {
    params.delete('from')
    params.delete('to')
  }
  if (settings.compare) {
    params.set('compare', '1')
  } else {
    params.delete('compare')
  }
  if (settings.exclude) {
    params.set('exclude_new_stations', '1')
  } else {
    params.delete('exclude_new_stations')
  }
  if (settings.resolution && settings.resolution !== DEFAULT_RESOLUTION) {
    params.set('resolution', settings.resolution)
  } else {
    params.delete('resolution')
  }
}

/// Defaults for a bare URL with no cookie: the week timeframe, compare off and
/// the mid resolution level.
const DEFAULTS: PersistedTimeframeSettings = {
  timeframe: 'week',
  compare: false,
  exclude: false,
  resolution: DEFAULT_RESOLUTION,
}

/// Default individual range when the user first picks it: the last 90 days.
function defaultIndividualRange(): { from: string; to: string } {
  const to = new Date()
  const from = new Date(to.getFullYear(), to.getMonth(), to.getDate() - 90)
  return { from: dateToInput(from), to: dateToInput(to) }
}

/// Shared timeframe/range/compare settings, persisted in the URL (primary) and
/// a cookie (fallback). Each page owns one instance bound to its own URL, so
/// the detail and summary pages keep independent, shareable views.
export function useTimeframeSettings(
  searchParams: URLSearchParams,
  setSearchParams: (params: URLSearchParams, options?: { replace?: boolean }) => void,
): TimeframeSettingsValue {
  const urlSettings = useMemo(() => parseUrl(searchParams), [searchParams])

  // A bare URL (no `timeframe` param) is re-populated from the cookie so the
  // settings are restored AND appear in the URL for sharing again. Re-runs
  // whenever the URL loses its `timeframe` param (e.g. an in-app navigation to
  // a new station). A fresh visitor (no cookie) is left on the defaults
  // implicitly — writing them into the URL here would change the route's
  // `location.key` and break the detail page's "Back to map" history heuristic.
  const urlHasTimeframe = searchParams.has('timeframe')
  useEffect(() => {
    if (urlHasTimeframe) return
    const restored = readCookie()
    if (!restored) return
    const params = new URLSearchParams(searchParams)
    applyToParams(params, restored)
    if (params.toString() !== searchParams.toString()) {
      setSearchParams(params, { replace: true })
    }
    setCookie(COOKIE_KEY, JSON.stringify(restored), COOKIE_MAX_AGE)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [urlHasTimeframe])

  // The effective settings: URL first, then the cookie fallback, then defaults.
  const effective: PersistedTimeframeSettings = urlSettings ?? readCookie() ?? DEFAULTS
  const isIndividual = effective.timeframe === 'individual'

  const update = useCallback(
    (next: PersistedTimeframeSettings) => {
      const params = new URLSearchParams(searchParams)
      applyToParams(params, next)
      setSearchParams(params, { replace: true })
      setCookie(COOKIE_KEY, JSON.stringify(next), COOKIE_MAX_AGE)
    },
    [searchParams, setSearchParams],
  )

  const setTimeframe = useCallback(
    (timeframe: Timeframe) => {
      if (timeframe === effective.timeframe) return
      if (timeframe === 'individual') {
        // Keep the current `compare` value (the box stays disabled but is
        // restored to this exact value when switching back to a fixed interval).
        const range =
          effective.timeframe === 'individual' && effective.from && effective.to
            ? { from: effective.from, to: effective.to }
            : defaultIndividualRange()
        update({ ...effective, timeframe, ...range })
      } else {
        update({ ...effective, timeframe, from: undefined, to: undefined })
      }
    },
    [effective, update],
  )

  const setFrom = useCallback(
    (from: string) => {
      if (!isIndividual) return
      const to = effective.to ?? defaultIndividualRange().to
      update({ ...effective, from, to })
    },
    [effective, isIndividual, update],
  )

  const setTo = useCallback(
    (to: string) => {
      if (!isIndividual) return
      const from = effective.from ?? defaultIndividualRange().from
      update({ ...effective, from, to })
    },
    [effective, isIndividual, update],
  )

  const setCompare = useCallback(
    (compare: boolean) => update({ ...effective, compare }),
    [effective, update],
  )

  const setExclude = useCallback(
    (exclude: boolean) => update({ ...effective, exclude }),
    [effective, update],
  )

  const setResolution = useCallback(
    (resolution: ResolutionLevel) => update({ ...effective, resolution }),
    [effective, update],
  )

  return {
    timeframe: effective.timeframe,
    from: isIndividual ? (effective.from ?? null) : null,
    to: isIndividual ? (effective.to ?? null) : null,
    compare: effective.compare,
    exclude: effective.exclude,
    resolution: effective.resolution,
    isIndividual,
    setTimeframe,
    setFrom,
    setTo,
    setCompare,
    setExclude,
    setResolution,
  }
}

/// The API query values for a custom from/to range: `from` at 00:00 UTC of the
/// start date and `to` at 00:00 UTC of the day AFTER the end date (an exclusive
/// upper bound, so the whole last day is included and the span equals the
/// selected day count — matching the backend granularity thresholds).
export function customRangeParams(from: string, to: string): URLSearchParams {
  const params = new URLSearchParams()
  params.set('from', `${from}T00:00:00Z`)
  const [year, month, day] = to.split('-').map(Number)
  const next = new Date(year, (month ?? 1) - 1, (day ?? 1) + 1)
  params.set('to', `${dateToInput(next)}T00:00:00Z`)
  return params
}

/// Appends the custom from/to range to a base graph URL (the `graphs_day` link
/// of the page shell serves as the base for the `individual` timeframe).
export function withCustomRange(baseUrl: string, from: string, to: string): string {
  const separator = baseUrl.includes('?') ? '&' : '?'
  return `${baseUrl}${separator}${customRangeParams(from, to).toString()}`
}
