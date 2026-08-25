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
import {
  formatFullDate,
  formatFullDateTime,
  formatNumber,
  formatTimestamp,
  LOCALE,
} from '../../lib/format'
import { serializeBounds, stationBounds } from '../../lib/geo'
import { SearchableHeader } from '../header/SearchableHeader'
import type { StationSummary } from '../stations/types'
import { MetricCard } from '../stationOverview/MetricCard'
import { useStationDetail } from './useStationDetail'
import type { ChannelRef, PeriodGraphs, StationDetail, Timeframe } from './types'
import { ChartCard } from './ChartCard'
import { ChannelPie } from './ChannelPie'
import { DetailMap } from './DetailMap'
import { MonthlyBarChart } from './MonthlyBarChart'
import { TimeSeriesLineChart, type LineSeries } from './TimeSeriesLineChart'
import { WeekdayRadar, type RadarSeries } from './WeekdayRadar'

type TimeUnit = 'hour' | 'day' | 'month'

/// Axis label for a bucket timestamp, tuned to the window's density. Labels use
/// the central `LOCALE` so a future locale change needs no per-chart edits.
function timeAxis(unit: TimeUnit): (time: number) => string {
  switch (unit) {
    case 'hour':
      return (time) =>
        new Date(time).toLocaleTimeString(LOCALE, { hour: '2-digit', minute: '2-digit' })
    case 'day':
      return (time) =>
        new Date(time).toLocaleDateString(LOCALE, { day: '2-digit', month: '2-digit' })
    case 'month':
      return (time) => new Date(time).toLocaleDateString(LOCALE, { month: 'short' })
  }
}

/// Axis label for the overlapped week chart: the aligned domain is anchored on a
/// Monday, so a short weekday name marks each day.
function weekdayAxis(time: number): string {
  return new Date(time).toLocaleDateString(LOCALE, { weekday: 'short' })
}

/// Browser-local midnight of the day containing `time` (period start for the day
/// and 30-day timeframes).
function dayStartOf(time: number): number {
  const date = new Date(time)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

/// Browser-local midnight of the Monday of the ISO week containing `time`.
function weekStartOf(time: number): number {
  const date = new Date(time)
  const sinceMonday = (date.getDay() + 6) % 7
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() - sinceMonday).getTime()
}

/// Browser-local midnight of Jan 1 of the year containing `time`.
function yearStartOf(time: number): number {
  const date = new Date(time)
  return new Date(date.getFullYear(), 0, 1).getTime()
}

/// Per-timeframe presentation config: labels, axis/tooltip formatters, the
/// period-start function used for the overlap alignment and the fixed axis
/// width. The year timeframe computes its domain to the actual next Jan 1.
interface TimeframeConfig {
  key: Timeframe
  label: string
  title: string
  subtitle: string
  radarSubtitle: string
  pieSubtitle: string
  perChannelTitle: string
  currentLabel: string
  previousLabel: string
  axis: (time: number) => string
  tooltip: (time: number) => string
  periodStart: (time: number) => number
  domainWidthMs?: number
}

const HOUR_MS = 3_600_000
const DAY_MS = 24 * HOUR_MS

const TIMEFRAMES: Record<Timeframe, TimeframeConfig> = {
  day: {
    key: 'day',
    label: '24 hours',
    title: '24 hours',
    subtitle: '5-minute buckets',
    radarSubtitle: 'the last complete day',
    pieSubtitle: 'over the last day',
    perChannelTitle: '24 hours by channel',
    currentLabel: 'Last day',
    previousLabel: 'Day before',
    axis: timeAxis('hour'),
    tooltip: formatFullDateTime,
    periodStart: dayStartOf,
    domainWidthMs: DAY_MS,
  },
  week: {
    key: 'week',
    label: 'Current + last week',
    title: 'Current + last week',
    subtitle: '1-hour buckets — the weeks are overlapped',
    radarSubtitle: 'the current week',
    pieSubtitle: 'over the current week',
    perChannelTitle: 'Current + last week by channel',
    currentLabel: 'Current week',
    previousLabel: 'Last week',
    axis: weekdayAxis,
    tooltip: formatFullDateTime,
    periodStart: weekStartOf,
    domainWidthMs: 7 * DAY_MS,
  },
  last_30_days: {
    key: 'last_30_days',
    label: 'Last 30 days',
    title: 'Last 30 days',
    subtitle: '1-day buckets',
    radarSubtitle: 'the last 30 days',
    pieSubtitle: 'over the last 30 days',
    perChannelTitle: 'Last 30 days by channel',
    currentLabel: 'Last 30 days',
    previousLabel: 'Previous 30 days',
    axis: timeAxis('day'),
    tooltip: formatFullDate,
    periodStart: dayStartOf,
    domainWidthMs: 30 * DAY_MS,
  },
  year: {
    key: 'year',
    label: 'Last year',
    title: 'Current + last year',
    subtitle: '1-day buckets — the years are overlapped',
    radarSubtitle: 'the current year',
    pieSubtitle: 'over the current year',
    perChannelTitle: 'Current + last year by channel',
    currentLabel: 'Current year',
    previousLabel: 'Last year',
    axis: timeAxis('month'),
    tooltip: formatFullDate,
    periodStart: yearStartOf,
  },
}

const TIMEFRAME_ORDER: Timeframe[] = ['day', 'week', 'last_30_days', 'year']

/// The x-axis domain of the selected timeframe, anchored on the current period's
/// start. The year timeframe runs to the actual next local Jan 1; the others use
/// a fixed width (DST days are a couple of minutes short/long, which is fine for
/// display).
function timeframeDomain(cfg: TimeframeConfig, anchor: number): [number, number] | undefined {
  if (!Number.isFinite(anchor)) return undefined
  if (cfg.key === 'year') {
    const yearEnd = new Date(new Date(anchor).getFullYear() + 1, 0, 1).getTime()
    return [anchor, yearEnd]
  }
  return [anchor, anchor + (cfg.domainWidthMs ?? 0)]
}

/// Shift every bucket to the same "position in period" axis so the current and
/// previous periods overlap: each series' own period start (from its first
/// bucket) is aligned onto the `anchor`. Returns the series unchanged when no
/// anchor is available (no data).
function alignSeries(
  series: LineSeries[],
  anchor: number,
  periodStart: (time: number) => number,
): LineSeries[] {
  if (!Number.isFinite(anchor)) return series
  return series.map((item) => {
    const first = item.data[0]
    if (!first) return item
    const windowStart = periodStart(new Date(first.start).getTime())
    if (!Number.isFinite(windowStart)) return item
    return {
      ...item,
      data: item.data.map((bucket) => {
        const time = new Date(bucket.start).getTime()
        return { ...bucket, start: new Date(anchor + (time - windowStart)).toISOString() }
      }),
    }
  })
}

/// Aggregate (all-channel) series for one timeframe: the current period plus the
/// previous period when the compare checkbox is on.
function timeframeSeries(period: PeriodGraphs, cfg: TimeframeConfig, compare: boolean): LineSeries[] {
  const series: LineSeries[] = []
  if (period.current.length > 0) {
    series.push({ key: 'current', label: cfg.currentLabel, data: period.current })
  }
  if (compare && period.previous.length > 0) {
    series.push({ key: 'previous', label: cfg.previousLabel, data: period.previous })
  }
  return series
}

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

          {!error && detail && <DetailContent detail={detail} />}
        </div>
      </main>
    </div>
  )
}

function DetailContent({ detail }: { detail: StationDetail }) {
  const { graphs, channels } = detail
  const [timeframe, setTimeframe] = useState<Timeframe>('day')
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
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <ChartCard title={cfg.perChannelTitle} subtitle={cfg.subtitle}>
            <TimeSeriesLineChart
              series={perChannelSeries}
              xFormatter={cfg.axis}
              tooltipFormatter={cfg.tooltip}
              xDomain={domain}
            />
          </ChartCard>
          <ChartCard title="Weekdays by channel" subtitle={cfg.radarSubtitle}>
            <WeekdayRadar series={channelRadar(period, channels)} />
          </ChartCard>
          <ChartCard title="Share by channel" subtitle={cfg.pieSubtitle}>
            <ChannelPie totals={period.channel_pie} channels={channels} />
          </ChartCard>
        </div>
      </section>
    </>
  )
}
