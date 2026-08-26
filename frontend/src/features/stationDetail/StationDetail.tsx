import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
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
import { serializeBounds, stationBounds } from '../../lib/geo'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import { SearchableHeader } from '../header/SearchableHeader'
import type { StationSummary } from '../stations/types'
import { MetricCard } from '../stationOverview/MetricCard'
import { useStationDetail } from './useStationDetail'
import type { ChannelRef, PeriodGraphs, StationDetail, Timeframe } from './types'
import {
  TIMEFRAMES,
  TIMEFRAME_ORDER,
  alignSeries,
  timeframeDomain,
  timeframeSeries,
  type TimeframeConfig,
} from './timeframes'
import { ChartCard } from './ChartCard'
import { ChannelPie } from './ChannelPie'
import { DetailMap } from './DetailMap'
import { MonthlyBarChart } from './MonthlyBarChart'
import { TimeSeriesLineChart, type LineSeries } from './TimeSeriesLineChart'
import { WeekdayRadar, type RadarSeries } from './WeekdayRadar'

/// Per-channel series for one timeframe: one line per channel, plus the previous
/// period per channel when the compare checkbox is on. Channels without data in
/// the requested periods are dropped.
function channelSeries(
  period: PeriodGraphs,
  cfg: TimeframeConfig,
  channels: ChannelRef[],
  compare: boolean,
): LineSeries[] {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  const series: LineSeries[] = []
  for (const channel of period.per_channel) {
    if (channel.current.length > 0) {
      series.push({
        key: `${channel.channel_id}_current`,
        label: `${nameOf(channel.channel_id)} (${cfg.currentLabel})`,
        data: channel.current,
      })
    }
    if (compare && channel.previous.length > 0) {
      series.push({
        key: `${channel.channel_id}_previous`,
        label: `${nameOf(channel.channel_id)} (${cfg.previousLabel})`,
        data: channel.previous,
      })
    }
  }
  return series
}

/// Per-channel weekday radar for one timeframe. Channels without any weekday
/// traffic are dropped.
function channelRadar(period: PeriodGraphs, channels: ChannelRef[]): RadarSeries[] {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  return period.per_channel
    .map((channel) => ({
      key: channel.channel_id,
      label: nameOf(channel.channel_id),
      data: channel.weekday_radar,
    }))
    .filter((series) => series.data.length > 0)
}

/// The counting-station detail page (`/stations/:id`): image + highlighted map
/// preview up top, then the overview stat boxes (same component as the overview
/// panel, plus the YEAR stat) and the graph sections. The shared header/search
/// stays active above the page.
export function StationDetail() {
  const { stationId } = useParams()
  const navigate = useNavigate()
  const { detail, error } = useStationDetail(stationId ?? null)

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
        <div className="mx-auto max-w-6xl px-4 py-6">
          <div className="mb-6 flex items-center justify-between gap-4">
            <Button asChild variant="outline" size="sm">
              <Link to="/">
                <ArrowLeft /> Back to map
              </Link>
            </Button>
            {error && (
              <span className="text-sm font-semibold text-destructive">
                Could not load the station.
              </span>
            )}
          </div>

          {!error && detail === null && (
            <p className="text-sm text-muted-foreground">Loading counting station…</p>
          )}

          {!error && detail && (
            <ErrorBoundary>
              <DetailContent detail={detail} />
            </ErrorBoundary>
          )}
        </div>
      </main>
    </div>
  )
}

function DetailContent({ detail }: { detail: StationDetail }) {
  const { graphs, channels } = detail
  // Default to the week timeframe ("This week"): the 24-hour window is not
  // always populated (e.g. a still-importing dataset), so a full week is the
  // safe default view.
  const [timeframe, setTimeframe] = useState<Timeframe>('week')
  const [comparePrevious, setComparePrevious] = useState(false)
  const cfg = TIMEFRAMES[timeframe]
  const period = graphs[timeframe]

  // Overlap anchor: the start of the current period (or the previous period when
  // the current one has no data yet), derived in the station's local day/week/
  // year grid so the current and previous periods can be overlaid.
  const firstBucket = period.current[0] ?? period.previous[0]
  const anchor = firstBucket ? cfg.periodStart(new Date(firstBucket.start).getTime()) : NaN
  const domain = timeframeDomain(cfg, anchor)

  const mainSeries = alignSeries(
    timeframeSeries(period, cfg, comparePrevious),
    anchor,
    cfg.periodStart,
  )
  const perChannelSeries = alignSeries(
    channelSeries(period, cfg, channels, comparePrevious),
    anchor,
    cfg.periodStart,
  )

  return (
    <>
      {/* Row 1: image (half the page) + highlighted map preview. */}
      <section className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <img
          src={detail.image_url}
          alt={`${detail.name} image`}
          className="h-64 w-full rounded-lg border object-cover md:h-80"
        />
        <DetailMap latitude={detail.latitude} longitude={detail.longitude} name={detail.name} />
      </section>

      {/* Row 2: name + description. */}
      <section className="mt-6">
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="text-2xl font-bold tracking-tight">{detail.name}</h1>
          <Badge variant="secondary">
            {formatNumber(detail.channel_count)} channel
            {detail.channel_count === 1 ? '' : 's'}
          </Badge>
        </div>
        {detail.description && (
          <p className="mt-2 text-sm text-muted-foreground">{detail.description}</p>
        )}
        <p className="mt-1 text-xs text-muted-foreground">
          Updated {formatTimestamp(detail.last_update)}
        </p>
      </section>

      {/* Overview stats: the same small boxes as the overview panel, + YEAR. */}
      <section className="mt-8">
        <h2 className="mb-3 text-lg font-semibold">Overview</h2>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {detail.metrics.map((metric) => (
            <MetricCard key={metric.key} metric={metric} />
          ))}
        </div>
      </section>

      {/* Detailed statistics: one shared timeframe selector driving the full-width
          line chart and the weekday radar. */}
      <section className="mt-8">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-semibold">Detailed statistics</h2>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground">Timeframe</span>
              <Select
                value={timeframe}
                onValueChange={(value) => setTimeframe(value as Timeframe)}
              >
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
          </div>
        </div>

        <div className="grid grid-cols-1 gap-4">
          <ChartCard title={cfg.title} subtitle={cfg.subtitle}>
            <TimeSeriesLineChart
              series={mainSeries}
              xFormatter={cfg.axis}
              tooltipFormatter={cfg.tooltip}
              xDomain={domain}
            />
          </ChartCard>
          <ChartCard title="Weekdays" subtitle={cfg.radarSubtitle}>
            <WeekdayRadar series={[{ key: 'total', label: 'Bikes', data: period.weekday_radar }]} />
          </ChartCard>
        </div>
      </section>

      {/* Monthly bar chart: all available months, standalone (not driven by the
          timeframe dropdown). */}
      <section className="mt-8">
        <MonthlyBarChart totals={graphs.monthly_totals} />
      </section>

      {/* Nerd stats: the same graphs per channel, all driven by the shared
          timeframe selector + compare checkbox. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Nerd stats</h2>
        <p className="mb-3 text-sm text-muted-foreground">The same graphs, drawn per channel.</p>
        <div className="grid grid-cols-1 gap-4">
          <ChartCard title={cfg.perChannelTitle} subtitle={cfg.subtitle}>
            <TimeSeriesLineChart
              series={perChannelSeries}
              xFormatter={cfg.axis}
              tooltipFormatter={cfg.tooltip}
              xDomain={domain}
              className="aspect-[21/9]"
            />
          </ChartCard>
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <ChartCard title="Weekdays by channel" subtitle={cfg.radarSubtitle}>
              <WeekdayRadar series={channelRadar(period, channels)} />
            </ChartCard>
            <ChartCard title="Share by channel" subtitle={cfg.pieSubtitle}>
              <ChannelPie totals={period.channel_pie} channels={channels} />
            </ChartCard>
          </div>
        </div>
      </section>
    </>
  )
}
