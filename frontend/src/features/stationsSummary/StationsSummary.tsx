import { useEffect, useMemo, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { Badge } from '@/components/ui/badge'
import { StationPage } from '../../components/StationPage'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { parseBoundsQuery, parseDisabled, serializeBounds } from '../../lib/geo'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import type { HourRadarSeries } from '../stationDetail/HourRadar'
import { SharePie, type ShareSlice } from '../stationDetail/SharePie'
import type { BarSeries } from '../stationDetail/TimeSeriesBarChart'
import { resolutionGranularity } from '../stationDetail/resolution'
import { alignSeries, timeframeSeries, type TimeframeConfig } from '../stationDetail/timeframes'
import type { RadarSeries } from '../stationDetail/WeekdayRadar'
import {
  graphsLink,
  monthlyBody,
  overviewBody,
  perSeriesStatsBody,
  statisticsBody,
  timeframeConfig,
} from '../stationDetail/sections'
import { useTrendSettings } from '../settings/TrendSettingsContext'
import { TimeframeSettingsControls } from '../settings/TimeframeSettingsControls'
import { useTimeframeSettings } from '../settings/useTimeframeSettings'
import { SummaryMap } from './SummaryMap'
import type { StationsSummaryPage, SummaryPeriodGraphs, SummaryStation } from './types'
import { useStationsSummaryGraphs } from './useStationsSummaryGraphs'
import { useStationsSummaryMonthly } from './useStationsSummaryMonthly'
import { useStationsSummaryOverview } from './useStationsSummaryOverview'
import { useStationsSummaryPage } from './useStationsSummaryPage'
import { PageShellSkeleton } from '../stationDetail/Skeletons'

/// Per-station series for one timeframe: one series per station (stacked into
/// the current-period bar), plus the previous period per station when the
/// compare checkbox is on (drawn as a separate side-by-side bar). The summary's
/// "Detailed stats" distinguish stations, not channels.
function stationSeries(
  period: SummaryPeriodGraphs,
  cfg: TimeframeConfig,
  stations: SummaryStation[],
  compare: boolean,
): BarSeries[] {
  const nameOf = (id: string) => stations.find((station) => station.id === id)?.name ?? id
  const series: BarSeries[] = []
  for (const station of period.per_station) {
    if (station.current.length > 0) {
      series.push({
        key: `${station.station_id}_current`,
        label: `${nameOf(station.station_id)} (${cfg.currentLabel})`,
        stackId: 'current',
        data: station.current,
      })
    }
    if (compare && station.previous.length > 0) {
      series.push({
        key: `${station.station_id}_previous`,
        label: `${nameOf(station.station_id)} (${cfg.previousLabel})`,
        stackId: 'previous',
        data: station.previous,
      })
    }
  }
  return series
}

/// Per-station weekday radar for one timeframe (detailed stats). Each station
/// with traffic contributes a current radar and, when compare is on, a previous
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

/// Per-station hour-of-day radar for one timeframe (detailed stats).
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

  return (
    <StationPage
      // Restore the exact map view by rebuilding /?<bounds> from the bounds
      // already in this /summary URL (fallback / when absent).
      backTo={bounds ? `/?${serializeBounds(bounds).toString()}` : '/'}
      backLabel="Back to map"
      errorMessage={error ? 'Could not load the station summary.' : undefined}
    >
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
    </StationPage>
  )
}

function SummaryContent({
  page,
  disabled,
  onToggle,
  bounds,
}: Readonly<{
  page: StationsSummaryPage
  disabled: Set<string>
  onToggle: (stationId: string) => void
  bounds: NonNullable<ReturnType<typeof parseBoundsQuery>>
}>) {
  const { stations } = page
  // The exclude set and the Bike-Trends flag are passed to the card hooks;
  // toggling either re-fetches only the dependent cards, never the shell.
  const disabledList = Array.from(disabled)
  const { excludeNewStations, setExcludeNewStations } = useTrendSettings()
  const [searchParams, setSearchParams] = useSearchParams()
  const settings = useTimeframeSettings(searchParams, setSearchParams)
  const { timeframe, from, to, compare, resolution, isIndividual } = settings

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
  // base) and derives its config from the selected range. The resolution level
  // maps to a concrete granularity per timeframe/range, which drives both the
  // chart config and the `resolution` query token on the link. Compare is
  // disabled for a custom range.
  const granularity = resolutionGranularity(resolution, timeframe, from, to)
  const cfg = timeframeConfig(isIndividual, timeframe, from, to, granularity)
  const graphLink = graphsLink(page._links, isIndividual, timeframe, from, to, granularity)
  const comparePrevious = compare && !isIndividual

  const { overview, error: overviewError } = useStationsSummaryOverview(
    page._links.overview,
    disabledList,
    excludeNewStations,
  )
  const { graphs, error: graphsError } = useStationsSummaryGraphs(
    graphLink,
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
  const anchor = firstBucket ? cfg.periodStart(new Date(firstBucket.start).getTime()) : Number.NaN

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
          alt="Station summary"
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
        {overviewBody(overview, overviewError)}
      </section>

      {/* Detailed statistics: shared timeframe selector driving the aggregate
          bar chart and the weekday radar. */}
      <section className="mt-8">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-semibold">Detailed statistics</h2>
          <TimeframeSettingsControls settings={settings} />
        </div>

        {statisticsBody(period, graphsError, cfg, mainSeries, comparePrevious)}
      </section>

      {/* Detailed stats card: the same graphs per station, driven by the shared
          timeframe selector + compare checkbox. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Detailed stats</h2>
        <p className="mb-3 text-sm text-muted-foreground">The same graphs, drawn per station.</p>
        {perSeriesStatsBody(period !== null, graphsError, {
          cfg,
          series: perStationSeries,
          weekdaysTitle: 'Weekdays by station',
          weekdays: period ? stationRadar(period, stations, cfg, comparePrevious) : [],
          hoursTitle: 'Hours by station',
          hours: period ? stationHourRadar(period, stations, cfg, comparePrevious) : [],
          shareTitle: 'Share by station',
          share: period ? <SharePie slices={stationSlices(period, stations)} /> : null,
        })}
      </section>

      {/* Monthly bar chart card: all available months, standalone (not driven by
          the timeframe dropdown), shown at the very bottom. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Bikes per month</h2>
        <p className="mb-3 text-sm text-muted-foreground">
          The setting applies to this chart too — stations added during the period are excluded.
        </p>
        {monthlyBody(monthly, monthlyError)}
      </section>
    </>
  )
}
