/// The Bike-Trends resolution changer: three levels (`high` / `mid` / `low`)
/// that map to a concrete bucket size per timeframe. The level is what the user
/// picks and what is persisted; the concrete granularity (the backend
/// `resolution` query token) is derived from the current timeframe/range so a
/// single persisted level keeps its meaning as the selection changes.
import type { FixedTimeframe, Timeframe } from './types'
import { dateFromInput, type GranularityKey } from './timeframes'

/// The three resolution levels offered by the settings dialog.
export type ResolutionLevel = 'high' | 'mid' | 'low'

/// Default level when nothing is persisted yet: the middle ground.
export const DEFAULT_RESOLUTION: ResolutionLevel = 'mid'

/// The canonical display order of the three levels.
export const RESOLUTION_LEVELS: ResolutionLevel[] = ['high', 'mid', 'low']

/// One selectable resolution: its level plus the concrete bucket size (the
/// label shown on the button and the `resolution` query token sent to the API).
export interface ResolutionOption {
  level: ResolutionLevel
  label: string
  granularity: GranularityKey
}

/// The three resolution levels per fixed timeframe, from finest (high) to
/// coarsest (low).
const FIXED_OPTIONS: Record<FixedTimeframe, ResolutionOption[]> = {
  day: [
    { level: 'high', label: '15 Min', granularity: '15m' },
    { level: 'mid', label: '30 Min', granularity: '30m' },
    { level: 'low', label: 'Hour', granularity: 'hour' },
  ],
  week: [
    { level: 'high', label: '30 Min', granularity: '30m' },
    { level: 'mid', label: 'Hour', granularity: 'hour' },
    { level: 'low', label: 'Day', granularity: 'day' },
  ],
  last_30_days: [
    { level: 'high', label: 'Hour', granularity: 'hour' },
    { level: 'mid', label: 'Day', granularity: 'day' },
    { level: 'low', label: 'Week', granularity: 'week' },
  ],
  year: [
    { level: 'high', label: 'Day', granularity: 'day' },
    { level: 'mid', label: 'Week', granularity: 'week' },
    { level: 'low', label: 'Month', granularity: 'month' },
  ],
}

/// The resolution options for an individual range spanning more than one year:
/// the year set with the `Day` option dropped (too fine for a multi-year
/// series), `Quarter` taking over the low slot.
const MULTI_YEAR_OPTIONS: ResolutionOption[] = [
  { level: 'high', label: 'Week', granularity: 'week' },
  { level: 'mid', label: 'Month', granularity: 'month' },
  { level: 'low', label: 'Quarter', granularity: 'quarter' },
]

/// Inclusive day count of a `YYYY-MM-DD` range (the frontend's span semantics:
/// from 00:00 of `from` to 00:00 of the day after `to`).
function rangeDays(from: string, to: string): number {
  const start = dateFromInput(from).getTime()
  const endExclusive = dateFromInput(to).getTime() + 86_400_000
  return Math.floor((endExclusive - start) / 86_400_000)
}

/// The three resolution options to show for the current selection. An
/// individual range picks the closest fixed timeframe's set by its span; a
/// range longer than one year drops the `Day` option.
export function resolutionOptions(
  timeframe: Timeframe,
  from: string | null,
  to: string | null,
): ResolutionOption[] {
  if (timeframe === 'individual') {
    if (from && to) {
      const days = rangeDays(from, to)
      if (days <= 2) return FIXED_OPTIONS.day
      if (days <= 7) return FIXED_OPTIONS.week
      if (days <= 45) return FIXED_OPTIONS.last_30_days
      if (days <= 366) return FIXED_OPTIONS.year
      return MULTI_YEAR_OPTIONS
    }
    // No usable range yet (individual without both dates): fall back to the
    // current default timeframe's set.
    return FIXED_OPTIONS.week
  }
  return FIXED_OPTIONS[timeframe]
}

/// The concrete granularity key of a resolution level for the current selection
/// (the `resolution` query parameter sent to the graphs API).
export function resolutionGranularity(
  level: ResolutionLevel,
  timeframe: Timeframe,
  from: string | null,
  to: string | null,
): GranularityKey {
  const options = resolutionOptions(timeframe, from, to)
  return (options.find((option) => option.level === level) ?? options[1]).granularity
}

/// Type guard for the persisted/URL resolution level.
export function isResolutionLevel(level: string | null | undefined): level is ResolutionLevel {
  return level === 'high' || level === 'mid' || level === 'low'
}

/// Appends the `resolution` granularity token to a graphs HATEOAS link (which
/// already carries `as_of` / bounds / the individual range).
export function withResolutionParam(url: string, granularity: GranularityKey): string {
  const separator = url.includes('?') ? '&' : '?'
  return `${url}${separator}resolution=${granularity}`
}
