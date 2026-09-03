import { useEffect, useState } from 'react'
import { Link, useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { ArrowLeft, SlidersHorizontal } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { serializeBounds, stationBounds } from '../../lib/geo'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import { SearchableHeader } from '../header/SearchableHeader'
import type { StationSummary } from '../stations/types'
import { useTrendSettings } from '../settings/TrendSettingsContext'
import { SettingsDialog } from '../settings/SettingsDialog'
import { TimeframeSettingsLabel } from '../settings/TimeframeSettingsLabel'
import { useTimeframeSettings, withCustomRange } from '../settings/useTimeframeSettings'
import { MetricCard } from '../stationOverview/MetricCard'
import { TotalBikesCard } from '../stationOverview/TotalBikesCard'
import type { ChannelRef, FixedTimeframe, PeriodGraphs, StationDetailPage } from './types'
import { GRAPH_LINK_KEYS } from './types'
import {
  TIMEFRAMES,
  alignSeries,
  customTimeframeConfig,
  dateFromInput,
  timeframeSeries,
  type TimeframeConfig,
} from './timeframes'
import { ChartCard } from './ChartCard'
import { ChannelPie } from './ChannelPie'
import { DetailMap } from './DetailMap'
import { HourRadar, type HourRadarSeries } from './HourRadar'
import { KeyFacts, computeKeyFacts } from './KeyFacts'
import { MonthlyBarChart } from './MonthlyBarChart'
import { TimeSeriesBarChart, type BarSeries } from './TimeSeriesBarChart'
import { WeekdayRadar, type RadarSeries } from './WeekdayRadar'
import { useStationDetailPage } from './useStationDetailPage'
import { useStationGraphs } from './useStationGraphs'
import { useStationMonthly } from './useStationMonthly'
import { useStationOverviewStats } from './useStationOverviewStats'
import {
  ChartsSkeleton,
  KeyFactsSkeleton,
  MonthlyBarSkeleton,
  OverviewSkeleton,
  PageShellSkeleton,
} from './Skeletons'

/// Per-channel series for one timeframe: one series per channel (stacked into
/// the current-period bar), plus the previous period per channel when the
/// compare checkbox is on (drawn as a separate side-by-side bar). Channels
/// without data in the requested periods are dropped.
function channelSeries(
  period: PeriodGraphs,
  cfg: TimeframeConfig,
  channels: ChannelRef[],
  compare: boolean,
): BarSeries[] {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  const series: BarSeries[] = []
  for (const channel of period.per_channel) {
    if (channel.current.length > 0) {
      series.push({
        key: `${channel.channel_id}_current`,
        label: `${nameOf(channel.channel_id)} (${cfg.currentLabel})`,
        stackId: 'current',
        data: channel.current,
      })
    }
    if (compare && channel.previous.length > 0) {
      series.push({
        key: `${channel.channel_id}_previous`,
        label: `${nameOf(channel.channel_id)} (${cfg.previousLabel})`,
        stackId: 'previous',
        data: channel.previous,
      })
    }
  }
  return series
}

/// Aggregate weekday radar for one timeframe: the current period's "Bikes" plus,
/// when the compare checkbox is on, the previous period's "Bikes".
function aggregateWeekdayRadar(
  period: PeriodGraphs,
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
function aggregateHourRadar(
  period: PeriodGraphs,
  cfg: TimeframeConfig,
  compare: boolean,
): HourRadarSeries[] {
  const series: HourRadarSeries[] = [{ key: 'current', label: 'Bikes', data: period.hourly }]
  if (compare && period.hourly_previous.length > 0) {
    series.push({ key: 'previous', label: cfg.previousLabel, data: period.hourly_previous })
  }
  return series
}

/// Per-channel weekday radar for one timeframe (detailed stats). Each channel
/// with traffic contributes a current radar and, when compare is on, a previous
/// period radar.
function channelRadar(
  period: PeriodGraphs,
  channels: ChannelRef[],
  cfg: TimeframeConfig,
  compare: boolean,
): RadarSeries[] {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  return period.per_channel.flatMap((channel) => {
    const series: RadarSeries[] = []
    if (channel.weekday_radar.length > 0) {
      series.push({
        key: `${channel.channel_id}_current`,
        label: nameOf(channel.channel_id),
        data: channel.weekday_radar,
      })
    }
    if (compare && channel.weekday_radar_previous.length > 0) {
      series.push({
        key: `${channel.channel_id}_previous`,
        label: `${nameOf(channel.channel_id)} (${cfg.previousLabel})`,
        data: channel.weekday_radar_previous,
      })
    }
    return series
  })
}

/// Per-channel hour-of-day radar for one timeframe (detailed stats).
function channelHourRadar(
  period: PeriodGraphs,
  channels: ChannelRef[],
  cfg: TimeframeConfig,
  compare: boolean,
): HourRadarSeries[] {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  return period.per_channel.flatMap((channel) => {
    const series: HourRadarSeries[] = []
    if (channel.hourly.length > 0) {
      series.push({
        key: `${channel.channel_id}_current`,
        label: nameOf(channel.channel_id),
        data: channel.hourly,
      })
    }
    if (compare && channel.hourly_previous.length > 0) {
      series.push({
        key: `${channel.channel_id}_previous`,
        label: `${nameOf(channel.channel_id)} (${cfg.previousLabel})`,
        data: channel.hourly_previous,
      })
    }
    return series
  })
}

/// The counting-station detail page (`/stations/:id`): the shell (image +
/// highlighted map preview + metadata) renders immediately, then each stats
/// card loads its own sub-resource via the shell's HATEOAS links. The shared
/// header/search stays active above the page.
export function StationDetail() {
  const { stationId } = useParams()
  const navigate = useNavigate()
  // When the page was reached through in-app navigation (not a shared/deep
  // link), the previous history entry is the map with its bounds + open
  // station; "Back to map" restores that exact view via history-back.
  const location = useLocation()
  const hasInAppHistory = location.key !== 'default'
  const { data: page, error } = useStationDetailPage(stationId ?? null)

  const openDetail = (station: StationSummary) => {
    navigate(`/stations/${station.id}`)
  }

  // "Find on map" from the detail page: go back to the map and fly to the
  // station by seeding a small bbox around it (plus the open-overview param).
  const findOnMap = (station: StationSummary) => {
    if (station.latitude !== null && station.longitude !== null) {
      const params = serializeBounds(stationBounds(station.latitude, station.longitude))
      params.set('station', station.id)
      navigate(`/?${params.toString()}`)
    } else {
      navigate(`/?station=${station.id}`)
    }
  }

  return (
    <div className="flex h-screen flex-col">
      <SearchableHeader onSelect={openDetail} onFind={findOnMap} onDetail={openDetail} />

      <main className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-6xl px-4 py-4 sm:py-6">
          <div className="mb-6 flex items-center justify-between gap-4">
            <Button asChild variant="outline" size="sm">
              {/* Restore the prior map view via history when we were reached by
                  in-app navigation; shared/deep links (location.key ===
                  'default') fall back to the plain map route. */}
              <Link
                to="/"
                onClick={(event) => {
                  if (hasInAppHistory) {
                    event.preventDefault()
                    navigate(-1)
                  }
                }}
              >
                <ArrowLeft /> Back to map
              </Link>
            </Button>
            {error && (
              <span className="text-sm font-semibold text-destructive">
                Could not load the station.
              </span>
            )}
          </div>

          {!error && page === null && <PageShellSkeleton />}

          {!error && page && (
            <ErrorBoundary>
              <DetailContent page={page} />
            </ErrorBoundary>
          )}
        </div>
      </main>
    </div>
  )
}

function DetailContent({ page }: { page: StationDetailPage }) {
  const { channels } = page
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [searchParams, setSearchParams] = useSearchParams()
  const settings = useTimeframeSettings(searchParams, setSearchParams)
  const { timeframe, from, to, compare, isIndividual } = settings
  const { excludeNewStations, setExcludeNewStations } = useTrendSettings()

  // A shared link carries the Bike-Trends flag (`exclude_new_stations=1`); sync
  // it into the app-global context so the header and every stats card agree with
  // the link. When the param is absent the localStorage value is left untouched.
  const urlExclude = searchParams.has('exclude_new_stations')
    ? searchParams.get('exclude_new_stations') === '1'
    : null
  useEffect(() => {
    if (urlExclude !== null) setExcludeNewStations(urlExclude)
  }, [urlExclude, setExcludeNewStations])

  // The fixed timeframes use their HATEOAS link and static config; the
  // individual range builds its own link (from/to appended to the `graphs_day`
  // base) and derives its config (labels/axis) from the selected range. Compare
  // is disabled for a custom range.
  const cfg: TimeframeConfig = !isIndividual
    ? TIMEFRAMES[timeframe as FixedTimeframe]
    : from && to
      ? customTimeframeConfig(dateFromInput(from), dateFromInput(to))
      : TIMEFRAMES.week
  const graphLink = !isIndividual
    ? page._links[GRAPH_LINK_KEYS[timeframe as FixedTimeframe]]
    : from && to
      ? withCustomRange(page._links.graphs_day, from, to)
      : page._links.graphs_week
  const comparePrevious = compare && !isIndividual

  // Each stats card fetches its own sub-resource through the shell's links
  // (which carry the `as_of` reference), so cards load and fail independently.
  // The Bike-Trends flag is appended to the windowed cards.
  const { data: overview, error: overviewError } = useStationOverviewStats(
    page._links.overview,
    excludeNewStations,
  )
  const { data: graphs, error: graphsError } = useStationGraphs(graphLink, excludeNewStations)
  const { data: monthly, error: monthlyError } = useStationMonthly(page._links.monthly)

  const period = graphs
  // Overlap anchor: the start of the current period (or the previous period when
  // the current one has no data yet), derived in the station's local day/week/
  // year grid so the current and previous periods can be overlaid.
  const firstBucket = period?.current[0] ?? period?.previous[0]
  const anchor = firstBucket ? cfg.periodStart(new Date(firstBucket.start).getTime()) : NaN

  const mainSeries = period
    ? alignSeries(timeframeSeries(period, cfg, comparePrevious), anchor, cfg.periodStart)
    : []
  const perChannelSeries = period
    ? alignSeries(channelSeries(period, cfg, channels, comparePrevious), anchor, cfg.periodStart)
    : []

  return (
    <>
      {/* Row 1: image (half the page) + highlighted map preview. */}
      <section className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <img
          src={page.image_url}
          alt={`${page.name} image`}
          className="h-64 w-full rounded-lg border object-cover md:h-80"
        />
        <DetailMap latitude={page.latitude} longitude={page.longitude} name={page.name} />
      </section>

      {/* Row 2: name + description. */}
      <section className="mt-6">
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="text-2xl font-bold tracking-tight">{page.name}</h1>
          <Badge variant="secondary">
            {formatNumber(page.channel_count)} channel
            {page.channel_count === 1 ? '' : 's'}
          </Badge>
        </div>
        {page.description && (
          <p className="mt-2 text-sm text-muted-foreground">{page.description}</p>
        )}
        <p className="mt-1 text-xs text-muted-foreground">
          Updated {formatTimestamp(page.last_update)}
        </p>
      </section>

      {/* Overview stats card: the all-time counter on top, then the same small
          boxes as the overview panel, + YEAR. */}
      <section className="mt-8">
        <h2 className="mb-3 text-lg font-semibold">Overview</h2>
        {overview ? (
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
        ) : overviewError ? (
          <p className="text-sm font-semibold text-destructive">Could not load the overview.</p>
        ) : (
          <OverviewSkeleton />
        )}
      </section>

      {/* Detailed statistics: one shared timeframe selector driving the full-width
          bar chart and the weekday radar. */}
      <section className="mt-8">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-semibold">Detailed statistics</h2>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <TimeframeSettingsLabel settings={settings} />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setSettingsOpen(true)}
              title="Calculation settings"
              aria-label="Calculation settings"
            >
              <SlidersHorizontal />
              Settings
            </Button>
            <SettingsDialog
              open={settingsOpen}
              onOpenChange={setSettingsOpen}
              settings={settings}
            />
          </div>
        </div>

        {/* Bike-Trends: with the setting on, a station opened during the period
            reports is_new — the previous-period comparison is not meaningful,
            so tell the user instead of drawing an empty overlay. */}
        {period?.is_new && (
          <p className="mb-3 text-sm text-muted-foreground">
            This station opened during the compared period — the previous period comparison is not
            shown.
          </p>
        )}

        {period ? (
          <div className="grid grid-cols-1 gap-4">
            {/* Key facts for the selected timeframe, in the overview's
                metric-box theme. */}
            <KeyFacts facts={computeKeyFacts(period)} />
            <ChartCard title={cfg.title} subtitle={cfg.subtitle}>
              <TimeSeriesBarChart
                series={mainSeries}
                xFormatter={cfg.axis}
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
        ) : graphsError ? (
          <p className="text-sm font-semibold text-destructive">Could not load the statistics.</p>
        ) : (
          <div className="flex flex-col gap-4">
            {/* The key facts are derived from the graphs card, so their boxes
                load (and fail) together with the charts. */}
            <KeyFactsSkeleton />
            <ChartsSkeleton />
          </div>
        )}
      </section>

      {/* Detailed stats card: the same graphs per channel, all driven by the
          shared timeframe selector + compare checkbox. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Detailed stats</h2>
        <p className="mb-3 text-sm text-muted-foreground">The same graphs, drawn per channel.</p>
        {period ? (
          <div className="grid grid-cols-1 gap-4">
            <ChartCard title={cfg.perChannelTitle} subtitle={cfg.subtitle}>
              <TimeSeriesBarChart
                series={perChannelSeries}
                xFormatter={cfg.axis}
                tooltipFormatter={cfg.tooltip}
                className="aspect-[21/18] sm:aspect-[21/9]"
              />
            </ChartCard>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <ChartCard title="Weekdays by channel" subtitle={cfg.radarSubtitle}>
                <WeekdayRadar series={channelRadar(period, channels, cfg, comparePrevious)} />
              </ChartCard>
              <ChartCard title="Hours by channel" subtitle={cfg.radarSubtitle}>
                <HourRadar series={channelHourRadar(period, channels, cfg, comparePrevious)} />
              </ChartCard>
            </div>
            {/* The share pie spans the full width so the donut + legend do not
                waste the second column of the two-radar row above. */}
            <ChartCard title="Share by channel" subtitle={cfg.pieSubtitle}>
              <ChannelPie totals={period.channel_pie} channels={channels} />
            </ChartCard>
          </div>
        ) : graphsError ? (
          <p className="text-sm font-semibold text-destructive">
            Could not load the detailed stats.
          </p>
        ) : (
          <ChartsSkeleton />
        )}
      </section>

      {/* Monthly bar chart card: all available months, standalone (not driven by
          the timeframe dropdown), shown at the very bottom. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Bikes per month</h2>
        <p className="mb-3 text-sm text-muted-foreground">
          The settings are not applied to this chart — newly added counting stations may add bikes.
        </p>
        {monthly ? (
          <MonthlyBarChart totals={monthly.monthly_totals} />
        ) : monthlyError ? (
          <p className="text-sm font-semibold text-destructive">
            Could not load the monthly totals.
          </p>
        ) : (
          <MonthlyBarSkeleton />
        )}
      </section>
    </>
  )
}
