import { LOCALE } from '../../lib/format'
import { TIMEFRAMES, dateFromInput } from '../stationDetail/timeframes'
import type { FixedTimeframe } from '../stationDetail/types'
import type { TimeframeSettingsValue } from './useTimeframeSettings'

/// Formats a `YYYY-MM-DD` input value as a short localized date (`31.12.1998`).
function formatDateInput(value: string): string {
  return dateFromInput(value).toLocaleDateString(LOCALE, {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  })
}

/// The short summary of the current timeframe settings, printed in the page
/// header left of the settings button (where the inline dropdown/checkbox used
/// to sit).
export function TimeframeSettingsLabel({ settings }: { settings: TimeframeSettingsValue }) {
  const { timeframe, from, to, compare, isIndividual } = settings

  let text: string
  if (isIndividual && from && to) {
    text = `Individual · ${formatDateInput(from)} – ${formatDateInput(to)}`
  } else {
    text = TIMEFRAMES[timeframe as FixedTimeframe]?.label ?? 'Individual'
    if (compare) {
      text += ' · Compare previous period'
    }
  }

  return <span className="text-sm text-muted-foreground">{text}</span>
}
