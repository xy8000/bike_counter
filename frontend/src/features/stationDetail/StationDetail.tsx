import { useEffect, useState } from 'react'
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { SlidersHorizontal } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { StationPage } from '@/components/StationPage'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import { useTrendSettings } from '../settings/TrendSettingsContext'
import { SettingsDialog } from '../settings/SettingsDialog'
import { TimeframeSettingsLabel } from '../settings/TimeframeSettingsLabel'
import { useTimeframeSettings } from '../settings/useTimeframeSettings'
import type { ChannelRef, PeriodGraphs, StationDetailPage } from './types'
import { resolutionGranularity } from './resolution'
import { alignSeries, timeframeSeries, type TimeframeConfig } from './timeframes'
import { ChannelPie } from './ChannelPie'
import { DetailMap } from './DetailMap'
import type { HourRadarSeries } from './HourRadar'
import type { BarSeries } from './TimeSeriesBarChart'
import type { RadarSeries } from './WeekdayRadar'
import { useStationDetailPage } from './useStationDetailPage'
import { useStationGraphs } from './useStationGraphs'
import { useStationMonthly } from './useStationMonthly'
import { useStationOverviewStats } from './useStationOverviewStats'
import { PageShellSkeleton } from './Skeletons'
import {
  graphsLink,
  monthlyBody,
  overviewBody,
  perSeriesStatsBody,
  statisticsBody,
  timeframeConfig,
} from './sections'

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

  return (
    <StationPage
      backTo="/"
      backLabel="Back to map"
      errorMessage={error ? 'Could not load the station.' : undefined}
      backOnClick={(event) => {
        // Shared/deep links (location.key === 'default') fall back to the plain
        // map route; in-app navigation restores the prior view via history.
        if (hasInAppHistory) {
          event.preventDefault()
          navigate(-1)
        }
      }}
    >
      {!error && page === null && <PageShellSkeleton />}

      {!error && page && (
        <ErrorBoundary>
          <DetailContent page={page} />
        </ErrorBoundary>
      )}
    </StationPage>
  )
}

function DetailContent({ page }: Readonly<{ page: StationDetailPage }>) {
  const { channels } = page
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [searchParams, setSearchParams] = useSearchParams()
  const settings = useTimeframeSettings(searchParams, setSearchParams)
  const { timeframe, from, to, compare, resolution, isIndividual } = settings
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
  // base) and derives its config (labels/axis) from the selected range. The
  // resolution level maps to a concrete granularity per timeframe/range, which
  // drives both the chart config and the `resolution` query token on the link.
  // Compare is disabled for a custom range.
  const granularity = resolutionGranularity(resolution, timeframe, from, to)
  const cfg = timeframeConfig(isIndividual, timeframe, from, to, granularity)
  const graphLink = graphsLink(page._links, isIndividual, timeframe, from, to, granularity)
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
  const anchor = firstBucket ? cfg.periodStart(new Date(firstBucket.start).getTime()) : Number.NaN

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
          alt={page.name}
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
        {overviewBody(overview, overviewError)}
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

        {statisticsBody(period, graphsError, cfg, mainSeries, comparePrevious)}
      </section>

      {/* Detailed stats card: the same graphs per channel, all driven by the
          shared timeframe selector + compare checkbox. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Detailed stats</h2>
        <p className="mb-3 text-sm text-muted-foreground">The same graphs, drawn per channel.</p>
        {perSeriesStatsBody(period !== null, graphsError, {
          cfg,
          series: perChannelSeries,
          weekdaysTitle: 'Weekdays by channel',
          weekdays: period ? channelRadar(period, channels, cfg, comparePrevious) : [],
          hoursTitle: 'Hours by channel',
          hours: period ? channelHourRadar(period, channels, cfg, comparePrevious) : [],
          shareTitle: 'Share by channel',
          share: period ? <ChannelPie totals={period.channel_pie} channels={channels} /> : null,
        })}
      </section>

      {/* Monthly bar chart card: all available months, standalone (not driven by
          the timeframe dropdown), shown at the very bottom. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Bikes per month</h2>
        <p className="mb-3 text-sm text-muted-foreground">
          The settings are not applied to this chart — newly added counting stations may add bikes.
        </p>
        {monthlyBody(monthly, monthlyError)}
      </section>
    </>
  )
}
