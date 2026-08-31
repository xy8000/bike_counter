import { useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router-dom'
import { ArrowLeft, SlidersHorizontal } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { parseBoundsQuery, parseDisabled, serializeBounds, stationBounds } from '../../lib/geo'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import { SearchableHeader } from '../header/SearchableHeader'
import type { StationSummary } from '../stations/types'
import { MetricCard } from '../stationOverview/MetricCard'
import { TotalBikesCard } from '../stationOverview/TotalBikesCard'
import { ChartCard } from '../stationDetail/ChartCard'
import { HourRadar, type HourRadarSeries } from '../stationDetail/HourRadar'
import { MonthlyBarChart } from '../stationDetail/MonthlyBarChart'
import { SharePie, type ShareSlice } from '../stationDetail/SharePie'
import { TimeSeriesLineChart, type LineSeries } from '../stationDetail/TimeSeriesLineChart'
import type { Timeframe } from '../stationDetail/types'
import {
  TIMEFRAMES,
  TIMEFRAME_ORDER,
  alignSeries,
  timeframeDomain,
  timeframeSeries,
  type TimeframeConfig,
} from '../stationDetail/timeframes'
import { WeekdayRadar, type RadarSeries } from '../stationDetail/WeekdayRadar'
import { useTrendSettings } from '../settings/TrendSettingsContext'
import { SettingsDialog } from '../settings/SettingsDialog'
import { SummaryMap } from './SummaryMap'
import { GRAPH_LINK_KEYS } from './types'
import type { StationsSummaryPage, SummaryPeriodGraphs, SummaryStation } from './types'
import { useStationsSummaryGraphs } from './useStationsSummaryGraphs'
import { useStationsSummaryMonthly } from './useStationsSummaryMonthly'
import { useStationsSummaryOverview } from './useStationsSummaryOverview'
import { useStationsSummaryPage } from './useStationsSummaryPage'
import {
  ChartsSkeleton,
  MonthlyBarSkeleton,
  OverviewSkeleton,
  PageShellSkeleton,
} from '../stationDetail/Skeletons'

/// Per-station series for one timeframe: one line per station, plus the previous
/// period per station when the compare checkbox is on (the summary's "Nerd
/// stats" distinguish stations, not channels).
function stationSeries(
  period: SummaryPeriodGraphs,
  cfg: TimeframeConfig,
  stations: SummaryStation[],
  compare: boolean,
): LineSeries[] {
  const nameOf = (id: string) => stations.find((station) => station.id === id)?.name ?? id
  const series: LineSeries[] = []
  for (const station of period.per_station) {
    if (station.current.length > 0) {
      series.push({
        key: `${station.station_id}_current`,
        label: `${nameOf(station.station_id)} (${cfg.currentLabel})`,
        data: station.current,
      })
    }
    if (compare && station.previous.length > 0) {
      series.push({
        key: `${station.station_id}_previous`,
        label: `${nameOf(station.station_id)} (${cfg.previousLabel})`,
        data: station.previous,
      })
    }
  }
  return series
}

/// Aggregate weekday radar for one timeframe: the current period's "Bikes" plus,
/// when the compare checkbox is on, the previous period's "Bikes".
function aggregateWeekdayRadar(
  period: SummaryPeriodGraphs,
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
  period: SummaryPeriodGraphs,
  cfg: TimeframeConfig,
  compare: boolean,
): HourRadarSeries[] {
  const series: HourRadarSeries[] = [{ key: 'current', label: 'Bikes', data: period.hourly }]
  if (compare && period.hourly_previous.length > 0) {
    series.push({ key: 'previous', label: cfg.previousLabel, data: period.hourly_previous })
  }
  return series
}

/// Per-station weekday radar for one timeframe (nerd stats). Each station with
/// traffic contributes a current radar and, when compare is on, a previous
/// period radar.
function stationRadar(
  period: SummaryPeriodGraphs,
  stations: SummaryStation[],
  cfg: TimeframeConfig,
  compare: boolean,
): RadarSeries[] {
  const nameOf = (id: string) => stations.find((station) => station.id === id)?.name ?? id
  return period.per_station.flatMap((station) => {
    const series: RadarSeries[] = []
    if (station.weekday_radar.length > 0) {
      series.push({
        key: `${station.station_id}_current`,
        label: nameOf(station.station_id),
        data: station.weekday_radar,
      })
    }
    if (compare && station.weekday_radar_previous.length > 0) {
      series.push({
        key: `${station.station_id}_previous`,
        label: `${nameOf(station.station_id)} (${cfg.previousLabel})`,
        data: station.weekday_radar_previous,
      })
    }
    return series
  })
}

/// Per-station hour-of-day radar for one timeframe (nerd stats).
function stationHourRadar(
  period: SummaryPeriodGraphs,
  stations: SummaryStation[],
  cfg: TimeframeConfig,
  compare: boolean,
): HourRadarSeries[] {
  const nameOf = (id: string) => stations.find((station) => station.id === id)?.name ?? id
  return period.per_station.flatMap((station) => {
    const series: HourRadarSeries[] = []
    if (station.hourly.length > 0) {
      series.push({
        key: `${station.station_id}_current`,
        label: nameOf(station.station_id),
        data: station.hourly,
      })
    }
    if (compare && station.hourly_previous.length > 0) {
      series.push({
        key: `${station.station_id}_previous`,
        label: `${nameOf(station.station_id)} (${cfg.previousLabel})`,
        data: station.hourly_previous,
      })
    }
    return series
  })
}

/// Per-station shares for the pie, mapped to the shared slice shape.
function stationSlices(period: SummaryPeriodGraphs, stations: SummaryStation[]): ShareSlice[] {
  const nameOf = (id: string) => stations.find((station) => station.id === id)?.name ?? id
  return period.station_pie.map((total) => ({
    id: total.station_id,
    name: nameOf(total.station_id),
    total: total.total,
  }))
}

/// The station-summary page (`/summary`): the shell (fallback image, interactive
/// map of the selected view, title) renders immediately, then each stats card
/// loads its own sub-resource via the shell's HATEOAS links. Toggling a map flag
/// re-fetches only the exclude-dependent cards (overview / graphs / monthly),
/// not the shell.
export function StationsSummary() {
  const [searchParams, setSearchParams] = useSearchParams()
  const navigate = useNavigate()
  // `parseBoundsQuery` builds a fresh object each call; memoize on the bound
  // values (not the whole search params) so toggling the `disabled`/`station`
  // params never re-fetches the shell.
  const boundsKey = [
    searchParams.get('min_lat'),
    searchParams.get('min_lng'),
    searchParams.get('max_lat'),
    searchParams.get('max_lng'),
  ].join(',')
  const bounds = useMemo(() => parseBoundsQuery(searchParams), [boundsKey]) // eslint-disable-line react-hooks/exhaustive-deps
  // Seeded from the URL once so a shared link restores the same selection;
  // toggling updates both the state and the `disabled` query param.
  const [disabled, setDisabled] = useState<string[]>(() => parseDisabled(searchParams))
  const { page, loading, error } = useStationsSummaryPage(bounds)

  // Mirror the disabled set into the URL (replace, so toggling does not spam the
  // history). The bounds are already in the URL from the map.
  useEffect(() => {
    const params = new URLSearchParams(searchParams)
    if (disabled.length > 0) {
      params.set('disabled', disabled.join(','))
    } else {
      params.delete('disabled')
    }
    const next = params.toString()
    if (next !== searchParams.toString()) {
      setSearchParams(params, { replace: true })
    }
  }, [disabled, searchParams, setSearchParams])

  const toggleStation = (stationId: string) => {
    setDisabled((current) =>
      current.includes(stationId)
        ? current.filter((id) => id !== stationId)
        : [...current, stationId],
    )
  }

  const openDetail = (station: StationSummary) => {
    navigate(`/stations/${station.id}`)
  }

  // "Find on map" from the summary page: go back to the map and fly to the
  // station by seeding a small bbox around it.
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
        <div className="mx-auto max-w-6xl px-4 py-6">
          <div className="mb-6 flex items-center justify-between gap-4">
            <Button asChild variant="outline" size="sm">
              {/* Restore the exact map view by rebuilding /?<bounds> from the
                  bounds already in this /summary URL (fallback / when absent). */}
              <Link to={bounds ? `/?${serializeBounds(bounds).toString()}` : '/'}>
                <ArrowLeft /> Back to map
              </Link>
            </Button>
            {error && (
              <span className="text-sm font-semibold text-destructive">
                Could not load the station summary.
              </span>
            )}
          </div>

          {!bounds && (
            <p className="text-sm text-muted-foreground">
              No map view selected. Go back to the map and summarize the visible stations.
            </p>
          )}

          {bounds && !error && loading && (
            <div aria-busy="true">
              <PageShellSkeleton />
            </div>
          )}

          {bounds && !error && !loading && page && (
            <ErrorBoundary>
              <SummaryContent
                page={page}
                disabled={new Set(disabled)}
                onToggle={toggleStation}
                bounds={bounds}
              />
            </ErrorBoundary>
          )}
        </div>
      </main>
    </div>
  )
}

function SummaryContent({
  page,
  disabled,
  onToggle,
  bounds,
}: {
  page: StationsSummaryPage
  disabled: Set<string>
  onToggle: (stationId: string) => void
  bounds: NonNullable<ReturnType<typeof parseBoundsQuery>>
}) {
  const { stations } = page
  // The exclude set and the Bike-Trends flag are passed to the card hooks;
  // toggling either re-fetches only the dependent cards, never the shell.
  const disabledList = Array.from(disabled)
  const { excludeNewStations } = useTrendSettings()
  // Default to the week timeframe, like the detail page.
  const [timeframe, setTimeframe] = useState<Timeframe>('week')
  const [comparePrevious, setComparePrevious] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const cfg = TIMEFRAMES[timeframe]

  const { overview, error: overviewError } = useStationsSummaryOverview(
    page._links.overview,
    disabledList,
    excludeNewStations,
  )
  const { graphs, error: graphsError } = useStationsSummaryGraphs(
    page._links[GRAPH_LINK_KEYS[timeframe]],
    disabledList,
    excludeNewStations,
  )
  const { monthly, error: monthlyError } = useStationsSummaryMonthly(
    page._links.monthly,
    disabledList,
    excludeNewStations,
  )

  const period = graphs
  const firstBucket = period?.current[0] ?? period?.previous[0]
  const anchor = firstBucket ? cfg.periodStart(new Date(firstBucket.start).getTime()) : NaN
  const domain = timeframeDomain(cfg, anchor)

  const mainSeries = period
    ? alignSeries(timeframeSeries(period, cfg, comparePrevious), anchor, cfg.periodStart)
    : []
  const perStationSeries = period
    ? alignSeries(stationSeries(period, cfg, stations, comparePrevious), anchor, cfg.periodStart)
    : []

  return (
    <>
      {/* Row 1: fallback image (half the page) + the interactive summary map. */}
      <section className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <img
          src={page.image_url}
          alt="Station summary image"
          className="h-64 w-full rounded-lg border object-cover md:h-80"
        />
        <SummaryMap stations={stations} disabled={disabled} onToggle={onToggle} bounds={bounds} />
      </section>

      {/* Row 2: title + counts. */}
      <section className="mt-6">
        <h1 className="text-2xl font-bold tracking-tight">Station summary</h1>
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <Badge variant="secondary">
            {formatNumber(stations.length)} station
            {stations.length === 1 ? '' : 's'}
          </Badge>
          {overview && (
            <Badge variant="secondary">
              {formatNumber(overview.channel_count)} channel
              {overview.channel_count === 1 ? '' : 's'}
            </Badge>
          )}
          <span className="text-xs text-muted-foreground">
            Updated {formatTimestamp(page.last_update)}
          </span>
        </div>
        <p className="mt-2 text-sm text-muted-foreground">
          Click a map flag to exclude a station from the charts.
        </p>
      </section>

      {/* Overview stats card: the aggregated all-time counter on top, then the
          same small boxes as the detail page. */}
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

      {/* Detailed statistics: shared timeframe selector driving the aggregate
          line chart and the weekday radar. */}
      <section className="mt-8">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-semibold">Detailed statistics</h2>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground">Timeframe</span>
              <Select value={timeframe} onValueChange={(value) => setTimeframe(value as Timeframe)}>
                <SelectTrigger className="w-[190px]" aria-label="Timeframe">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TIMEFRAME_ORDER.map((key) => (
                    <SelectItem key={key} value={key}>
                      {TIMEFRAMES[key].label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center gap-2">
              <Checkbox
                id="compare-previous"
                checked={comparePrevious}
                onCheckedChange={(checked) => setComparePrevious(checked === true)}
              />
              <Label htmlFor="compare-previous">Compare previous period</Label>
            </div>
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
            <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
          </div>
        </div>

        {period ? (
          <div className="grid grid-cols-1 gap-4">
            <ChartCard title={cfg.title} subtitle={cfg.subtitle}>
              <TimeSeriesLineChart
                series={mainSeries}
                xFormatter={cfg.axis}
                tooltipFormatter={cfg.tooltip}
                xDomain={domain}
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
          <ChartsSkeleton />
        )}
      </section>

      {/* Monthly bar chart card: all available months, standalone. */}
      <section className="mt-8">
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

      {/* Nerd stats card: the same graphs per station, driven by the shared
          timeframe selector + compare checkbox. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Nerd stats</h2>
        <p className="mb-3 text-sm text-muted-foreground">The same graphs, drawn per station.</p>
        {period ? (
          <div className="grid grid-cols-1 gap-4">
            <ChartCard title={cfg.perChannelTitle} subtitle={cfg.subtitle}>
              <TimeSeriesLineChart
                series={perStationSeries}
                xFormatter={cfg.axis}
                tooltipFormatter={cfg.tooltip}
                xDomain={domain}
                className="aspect-[21/9]"
              />
            </ChartCard>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <ChartCard title="Weekdays by station" subtitle={cfg.radarSubtitle}>
                <WeekdayRadar series={stationRadar(period, stations, cfg, comparePrevious)} />
              </ChartCard>
              <ChartCard title="Hours by station" subtitle={cfg.radarSubtitle}>
                <HourRadar series={stationHourRadar(period, stations, cfg, comparePrevious)} />
              </ChartCard>
            </div>
            {/* The share pie spans the full width so the donut + legend do not
                waste the second column of the two-radar row above. */}
            <ChartCard title="Share by station" subtitle={cfg.pieSubtitle}>
              <SharePie slices={stationSlices(period, stations)} />
            </ChartCard>
          </div>
        ) : graphsError ? (
          <p className="text-sm font-semibold text-destructive">Could not load the nerd stats.</p>
        ) : (
          <ChartsSkeleton />
        )}
      </section>
    </>
  )
}
