/// Chart-section helpers shared by the station-detail and station-summary pages.
///
/// The two pages render the same "Overview", "Detailed statistics", "Detailed
/// stats" and "Bikes per month" sections; keeping the section bodies (and the
/// timeframe/link/radar helpers behind them) here avoids maintaining two near
/// identical copies.
import type { ReactNode } from 'react'
import { MetricCard } from '../stationOverview/MetricCard'
import type { StationOverviewMetric } from '../stationOverview/types'
import { TotalBikesCard } from '../stationOverview/TotalBikesCard'
import { withCustomRange } from '../settings/useTimeframeSettings'
import { ChartCard } from './ChartCard'
import { HourRadar, type HourRadarSeries } from './HourRadar'
import { KeyFacts, computeKeyFacts, type KeyFactsInput } from './KeyFacts'
import { MonthlyBarChart } from './MonthlyBarChart'
import { withResolutionParam } from './resolution'
import { ChartsSkeleton, KeyFactsSkeleton, MonthlyBarSkeleton, OverviewSkeleton } from './Skeletons'
import { TimeSeriesBarChart, type BarSeries } from './TimeSeriesBarChart'
import { WeekdayRadar, type RadarSeries } from './WeekdayRadar'
import {
  TIMEFRAMES,
  customTimeframeConfig,
  fixedTimeframeConfig,
  type GranularityKey,
  type TimeframeConfig,
} from './timeframes'
import {
  GRAPH_LINK_KEYS,
  type DetailLinks,
  type FixedTimeframe,
  type HourTotal,
  type MonthTotal,
  type Timeframe,
  type WeekdayTotal,
} from './types'

/// The overview-card shape shared by the detail and summary pages.
export interface OverviewCards {
  total_bikes: number
  metrics: StationOverviewMetric[]
}

/// The aggregate time-series fields the radar helpers read; both the detail
/// `PeriodGraphs` and the summary `SummaryPeriodGraphs` expose them.
export interface RadarPeriod {
  weekday_radar: WeekdayTotal[]
  weekday_radar_previous: WeekdayTotal[]
  hourly: HourTotal[]
  hourly_previous: HourTotal[]
}

/// The graphs HATEOAS links both shells expose. `GRAPH_LINK_KEYS` maps a fixed
/// timeframe to a key of the shared `DetailLinks` shape, so indexing has to be
/// keyed by that full shape (the summary links are a structural subset).
type GraphLinks = Record<keyof DetailLinks, string>

/// The timeframe presentation config for the current selection: the fixed
/// timeframes use their static config; an individual range derives its own, or
/// falls back to the week preset until both dates are set.
export function timeframeConfig(
  isIndividual: boolean,
  timeframe: Timeframe,
  from: string | null,
  to: string | null,
  granularity: GranularityKey,
): TimeframeConfig {
  if (!isIndividual) return fixedTimeframeConfig(timeframe as FixedTimeframe, granularity)
  if (from && to) return customTimeframeConfig(granularity, from, to)
  return TIMEFRAMES.week
}

/// The graphs HATEOAS link for the current selection, with the resolution token
/// appended.
export function graphsLink(
  links: GraphLinks,
  isIndividual: boolean,
  timeframe: Timeframe,
  from: string | null,
  to: string | null,
  granularity: GranularityKey,
): string {
  let base: string
  if (!isIndividual) {
    base = links[GRAPH_LINK_KEYS[timeframe as FixedTimeframe]]
  } else if (from && to) {
    base = withCustomRange(links.graphs_day, from, to)
  } else {
    base = links.graphs_week
  }
  return withResolutionParam(base, granularity)
}

/// Aggregate weekday radar for one timeframe: the current period's "Bikes" plus,
/// when the compare checkbox is on, the previous period's "Bikes".
export function aggregateWeekdayRadar(
  period: RadarPeriod,
  cfg: TimeframeConfig,
  compare: boolean,
): RadarSeries[] {
  const series: RadarSeries[] = [{ key: 'current', label: 'Bikes', data: period.weekday_radar }]
  if (compare && period.weekday_radar_previous.length > 0) {
    series.push({ key: 'previous', label: cfg.previousLabel, data: period.weekday_radar_previous })
  }
  return series
}

/// Aggregate hour-of-day radar for one timeframe: the current period's "Bikes"
/// plus, when the compare checkbox is on, the previous period's "Bikes".
export function aggregateHourRadar(
  period: RadarPeriod,
  cfg: TimeframeConfig,
  compare: boolean,
): HourRadarSeries[] {
  const series: HourRadarSeries[] = [{ key: 'current', label: 'Bikes', data: period.hourly }]
  if (compare && period.hourly_previous.length > 0) {
    series.push({ key: 'previous', label: cfg.previousLabel, data: period.hourly_previous })
  }
  return series
}

/// The overview stats card body: total bikes + metrics, an error line or the
/// loading skeleton.
export function overviewBody(overview: OverviewCards | null, overviewError: boolean): ReactNode {
  if (overview) {
    return (
      <>
        <div className="mb-3">
          <TotalBikesCard total={overview.total_bikes} />
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {overview.metrics.map((metric) => (
            <MetricCard key={metric.key} metric={metric} />
          ))}
        </div>
      </>
    )
  }
  if (overviewError) {
    return <p className="text-sm font-semibold text-destructive">Could not load the overview.</p>
  }
  return <OverviewSkeleton />
}

/// The main charts body for the selected timeframe (key facts + bar chart + the
/// two radars), an error line or the loading skeleton.
export function statisticsBody(
  period: (KeyFactsInput & RadarPeriod) | null,
  graphsError: boolean,
  cfg: TimeframeConfig,
  mainSeries: BarSeries[],
  comparePrevious: boolean,
): ReactNode {
  if (!period) {
    if (graphsError) {
      return (
        <p className="text-sm font-semibold text-destructive">Could not load the statistics.</p>
      )
    }
    return (
      <div className="flex flex-col gap-4">
        <KeyFactsSkeleton />
        <ChartsSkeleton />
      </div>
    )
  }
  return (
    <div className="grid grid-cols-1 gap-4">
      <KeyFacts facts={computeKeyFacts(period)} />
      <ChartCard title={cfg.title} subtitle={cfg.subtitle}>
        <TimeSeriesBarChart
          series={mainSeries}
          xFormatter={cfg.axis}
          axisRotate={cfg.axisRotate}
          tooltipFormatter={cfg.tooltip}
          className="aspect-[20/15.3] sm:aspect-[20/7.65]"
        />
      </ChartCard>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <ChartCard title="Weekdays" subtitle={cfg.radarSubtitle}>
          <WeekdayRadar series={aggregateWeekdayRadar(period, cfg, comparePrevious)} />
        </ChartCard>
        <ChartCard title="Hours" subtitle={cfg.radarSubtitle}>
          <HourRadar series={aggregateHourRadar(period, cfg, comparePrevious)} />
        </ChartCard>
      </div>
    </div>
  )
}

/// The detailed "per channel / per station" stats section: the same charts as
/// the aggregate section but split per series, plus the share pie.
export interface PerSeriesSection {
  cfg: TimeframeConfig
  series: BarSeries[]
  weekdaysTitle: string
  weekdays: RadarSeries[]
  hoursTitle: string
  hours: HourRadarSeries[]
  shareTitle: string
  share: ReactNode
}

/// The per-series stats body, an error line or the loading skeleton.
export function perSeriesStatsBody(
  loaded: boolean,
  graphsError: boolean,
  section: PerSeriesSection,
): ReactNode {
  if (!loaded) {
    if (graphsError) {
      return (
        <p className="text-sm font-semibold text-destructive">Could not load the detailed stats.</p>
      )
    }
    return <ChartsSkeleton />
  }
  const { cfg, series, weekdaysTitle, weekdays, hoursTitle, hours, shareTitle, share } = section
  return (
    <div className="grid grid-cols-1 gap-4">
      <ChartCard title={cfg.perChannelTitle} subtitle={cfg.subtitle}>
        <TimeSeriesBarChart
          series={series}
          xFormatter={cfg.axis}
          axisRotate={cfg.axisRotate}
          tooltipFormatter={cfg.tooltip}
          className="aspect-[21/18] sm:aspect-[21/9]"
        />
      </ChartCard>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <ChartCard title={weekdaysTitle} subtitle={cfg.radarSubtitle}>
          <WeekdayRadar series={weekdays} />
        </ChartCard>
        <ChartCard title={hoursTitle} subtitle={cfg.radarSubtitle}>
          <HourRadar series={hours} />
        </ChartCard>
      </div>
      {/* The share pie spans the full width so the donut + legend do not waste
          the second column of the two-radar row above. */}
      <ChartCard title={shareTitle} subtitle={cfg.pieSubtitle}>
        {share}
      </ChartCard>
    </div>
  )
}

/// The monthly totals body, an error line or the loading skeleton.
export function monthlyBody(
  monthly: { monthly_totals: MonthTotal[] } | null,
  monthlyError: boolean,
): ReactNode {
  if (monthly) return <MonthlyBarChart totals={monthly.monthly_totals} />
  if (monthlyError) {
    return (
      <p className="text-sm font-semibold text-destructive">Could not load the monthly totals.</p>
    )
  }
  return <MonthlyBarSkeleton />
}
