import { Link, useParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { MetricCard } from '../stationOverview/MetricCard'
import { useStationDetail } from './useStationDetail'
import type { StationDetail } from './types'
import { ChartCard } from './ChartCard'
import { ChannelPie } from './ChannelPie'
import { DetailMap } from './DetailMap'
import { TimeSeriesLineChart, type LineSeries } from './TimeSeriesLineChart'
import { WeekdayRadar, type RadarSeries } from './WeekdayRadar'

type TimeUnit = 'hour' | 'day' | 'month'

/// Axis/tooltip label for a bucket timestamp, tuned to the window's density.
function timeAxis(unit: TimeUnit): (time: number) => string {
  switch (unit) {
    case 'hour':
      return (time) =>
        new Date(time).toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' })
    case 'day':
      return (time) =>
        new Date(time).toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit' })
    case 'month':
      return (time) => new Date(time).toLocaleDateString('de-DE', { month: 'short' })
  }
}

type TimeWindow =
  | 'last_day'
  | 'current_week'
  | 'last_week'
  | 'last_30_days'
  | 'current_year'
  | 'last_year'

/// Aggregate (all-channel) series for one named window.
function aggregateSeries(graphs: StationDetail['graphs'], window: TimeWindow): LineSeries[] {
  return [{ key: 'total', label: 'Bikes', data: graphs[window] }]
}

/// Per-channel series (nerd stats): one line per channel, overlaid on a shared
/// time axis. Channels without data still get a series so the legend is complete.
function channelSeries(detail: StationDetail, windows: TimeWindow[]): LineSeries[] {
  return detail.channels.map((channel) => {
    const series = detail.graphs.per_channel.find((p) => p.channel_id === channel.id)
    const data = windows.flatMap((window) => series?.[window] ?? [])
    return { key: channel.id, label: channel.name, data }
  })
}

/// Per-channel weekday radar (nerd stats).
function channelRadar(detail: StationDetail): RadarSeries[] {
  return detail.channels.map((channel) => ({
    key: channel.id,
    label: channel.name,
    data:
      detail.graphs.per_channel.find((p) => p.channel_id === channel.id)?.weekday_radar ?? [],
  }))
}

/// The counting-station detail page (`/stations/:id`): image + highlighted map
/// preview up top, then the overview stat boxes (same component as the overview
/// panel, plus the YEAR stat) and the graph sections.
export function StationDetail() {
  const { stationId } = useParams()
  const { detail, error } = useStationDetail(stationId ?? null)

  return (
    <main className="h-screen overflow-y-auto">
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
  )
}

function DetailContent({ detail }: { detail: StationDetail }) {
  const { graphs } = detail

  // The current-week window for the "current vs last week" charts: from the
  // previous Monday to the end of the current week, so the running week's empty
  // tail is visible without fabricating buckets.
  const weekDomain: [number, number] | undefined = (() => {
    const first = graphs.current_week[0]
    if (!first) return undefined
    const weekStart = new Date(first.start).getTime()
    return [weekStart - 7 * 86_400_000, weekStart + 7 * 86_400_000]
  })()

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

      {/* Detailed statistics: one graph per half page. */}
      <section className="mt-8">
        <h2 className="mb-3 text-lg font-semibold">Detailed statistics</h2>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <ChartCard title="Last 24 hours" subtitle="5-minute buckets">
            <TimeSeriesLineChart
              series={aggregateSeries(graphs, 'last_day')}
              xFormatter={timeAxis('hour')}
            />
          </ChartCard>
          <ChartCard title="Weekdays" subtitle="last 30 days">
            <WeekdayRadar series={[{ key: 'total', label: 'Bikes', data: graphs.weekday_radar }]} />
          </ChartCard>
          <ChartCard
            title="Current week vs. last week"
            subtitle="15-minute buckets — the current week ends at the latest data"
          >
            <TimeSeriesLineChart
              series={[
                { key: 'current_week', label: 'Current week', data: graphs.current_week },
                { key: 'last_week', label: 'Last week', data: graphs.last_week },
              ]}
              xDomain={weekDomain}
              xFormatter={timeAxis('day')}
            />
          </ChartCard>
          <ChartCard
            title="Last 30 days"
            subtitle="30-minute buckets"
            note="The weekday radar and the channel share also cover the last 30 days."
          >
            <TimeSeriesLineChart
              series={aggregateSeries(graphs, 'last_30_days')}
              xFormatter={timeAxis('day')}
            />
          </ChartCard>
          <ChartCard title="Current year vs. last year" subtitle="1-day buckets">
            <TimeSeriesLineChart
              series={[
                { key: 'current_year', label: 'Current year', data: graphs.current_year },
                { key: 'last_year', label: 'Last year', data: graphs.last_year },
              ]}
              xFormatter={timeAxis('month')}
            />
          </ChartCard>
        </div>
      </section>

      {/* Nerd stats: the same graphs split by channel + the channel pie. */}
      <section className="mt-8">
        <h2 className="mb-1 text-lg font-semibold">Nerd stats</h2>
        <p className="mb-3 text-sm text-muted-foreground">The same graphs, drawn per channel.</p>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <ChartCard title="Last 24 hours by channel" subtitle="5-minute buckets">
            <TimeSeriesLineChart
              series={channelSeries(detail, ['last_day'])}
              xFormatter={timeAxis('hour')}
            />
          </ChartCard>
          <ChartCard title="Weekdays by channel" subtitle="last 30 days">
            <WeekdayRadar series={channelRadar(detail)} />
          </ChartCard>
          <ChartCard
            title="Current + last week by channel"
            subtitle="15-minute buckets"
          >
            <TimeSeriesLineChart
              series={channelSeries(detail, ['current_week', 'last_week'])}
              xDomain={weekDomain}
              xFormatter={timeAxis('day')}
            />
          </ChartCard>
          <ChartCard title="Last 30 days by channel" subtitle="30-minute buckets">
            <TimeSeriesLineChart
              series={channelSeries(detail, ['last_30_days'])}
              xFormatter={timeAxis('day')}
            />
          </ChartCard>
          <ChartCard title="Current + last year by channel" subtitle="1-day buckets">
            <TimeSeriesLineChart
              series={channelSeries(detail, ['current_year', 'last_year'])}
              xFormatter={timeAxis('month')}
            />
          </ChartCard>
          <ChartCard title="Share by channel" subtitle="last 30 days">
            <ChannelPie totals={graphs.channel_pie} channels={detail.channels} />
          </ChartCard>
        </div>
      </section>
    </>
  )
}
