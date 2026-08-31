import { formatFullDate, formatNumber, LOCALE } from '../../lib/format'
import type { HourTotal, TimeBucket, WeekdayTotal } from './types'

/// The slice of the graph card the key facts are derived from (both the detail
/// `PeriodGraphs` and the summary `SummaryPeriodGraphs` expose these fields).
export interface KeyFactsInput {
  current: TimeBucket[]
  hourly: HourTotal[]
  weekday_radar: WeekdayTotal[]
}

/// One key-fact box: a label, a large value, an optional unit and an optional
/// detail line (e.g. the bike count behind the busiest day/hour/weekday).
export interface KeyFact {
  key: string
  label: string
  value: string
  unit?: string
  detail?: string
}

/// The browser-local calendar-day key of a bucket's ISO-8601 start, matching how
/// the charts align buckets to local time.
function localDayKey(iso: string): string {
  const date = new Date(iso)
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`
}

/// Rebuilds a browser-local `Date` from a `localDayKey`.
function dateFromDayKey(key: string): Date {
  const [year, month, day] = key.split('-').map(Number)
  return new Date(year, month, day)
}

/// The busiest calendar day of the current period: buckets are summed into
/// browser-local days and the day with the highest total wins. This is exact
/// for daily-or-finer buckets (5-min / hour / day); for coarser custom ranges
/// (week / month / quarter buckets) each bucket is attributed to its start day,
/// which is the most meaningful single day the data can express.
function busiestDay(current: TimeBucket[]): { date: Date; total: number } | null {
  const byDay = new Map<string, number>()
  for (const bucket of current) {
    const key = localDayKey(bucket.start)
    byDay.set(key, (byDay.get(key) ?? 0) + bucket.total)
  }
  let bestKey: string | null = null
  let bestTotal = 0
  for (const [key, total] of byDay) {
    if (bestKey === null || total > bestTotal) {
      bestKey = key
      bestTotal = total
    }
  }
  return bestKey === null ? null : { date: dateFromDayKey(bestKey), total: bestTotal }
}

/// The peak hour-of-day total (`null` when there is no traffic at all).
function busiestHour(hourly: HourTotal[]): { hour: number; total: number } | null {
  const peak = hourly.reduce<HourTotal | null>(
    (best, entry) => (best === null || entry.total > best.total ? entry : best),
    null,
  )
  return peak && peak.total > 0 ? { hour: peak.hour, total: peak.total } : null
}

/// The peak weekday total (ISO 1 = Monday .. 7 = Sunday); `null` when there is
/// no traffic at all.
function busiestWeekday(weekday_radar: WeekdayTotal[]): { weekday: number; total: number } | null {
  const peak = weekday_radar.reduce<WeekdayTotal | null>(
    (best, entry) => (best === null || entry.total > best.total ? entry : best),
    null,
  )
  return peak && peak.total > 0 ? { weekday: peak.weekday, total: peak.total } : null
}

/// Formats a 0-23 hour as a locale `HH:00` value.
function hourLabel(hour: number): string {
  return new Date(0, 0, 0, hour).toLocaleTimeString(LOCALE, {
    hour: '2-digit',
    minute: '2-digit',
  })
}

/// The locale long weekday name of an ISO weekday (1 = Monday).
function weekdayLabel(weekday: number): string {
  // 2021-01-04 is a Monday, so ISO weekday 1..7 maps onto that week.
  const date = new Date(2021, 0, 4 + (weekday - 1))
  return date.toLocaleDateString(LOCALE, { weekday: 'long' })
}

/// Derives the key facts for the selected timeframe from the graph card. Facts
/// whose input is empty are skipped, and no minimum values are ever shown
/// (empty days/hours would read 0).
export function computeKeyFacts(period: KeyFactsInput): KeyFact[] {
  const facts: KeyFact[] = []

  if (period.current.length > 0) {
    const total = period.current.reduce((acc, bucket) => acc + bucket.total, 0)
    facts.push({
      key: 'total_bikes',
      label: 'Total bikes in selection',
      value: formatNumber(total),
      unit: 'bikes',
    })
  }

  const day = busiestDay(period.current)
  if (day !== null) {
    facts.push({
      key: 'busiest_day',
      label: 'Busiest day in range',
      value: formatFullDate(day.date.getTime()),
      detail: `${formatNumber(day.total)} bikes`,
    })
  }

  const peakHour = busiestHour(period.hourly)
  if (peakHour) {
    facts.push({
      key: 'busiest_hour',
      label: 'Busiest hour',
      value: hourLabel(peakHour.hour),
      detail: `${formatNumber(peakHour.total)} bikes`,
    })
  }

  const peakWeekday = busiestWeekday(period.weekday_radar)
  if (peakWeekday) {
    facts.push({
      key: 'busiest_weekday',
      label: 'Busiest weekday',
      value: weekdayLabel(peakWeekday.weekday),
      detail: `${formatNumber(peakWeekday.total)} bikes`,
    })
  }

  return facts
}

/// The key-facts row for the "Detailed statistics" section, styled like the
/// overview's `MetricCard` boxes (bordered box: label, large value, unit or
/// detail line) and laid out in the same four-column grid.
export function KeyFacts({ facts }: { facts: KeyFact[] }) {
  if (facts.length === 0) return null
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
      {facts.map((fact) => (
        <div key={fact.key} className="rounded-md border p-3">
          <p className="text-sm font-medium">{fact.label}</p>
          <p className="text-2xl font-semibold leading-tight">
            {fact.value}
            {fact.unit && (
              <span className="ml-1 text-xs font-normal text-muted-foreground">{fact.unit}</span>
            )}
          </p>
          {fact.detail && <p className="mt-1 text-xs text-muted-foreground">{fact.detail}</p>}
        </div>
      ))}
    </div>
  )
}
