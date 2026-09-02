import { useEffect, useState, type ReactNode } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { AlertTriangle, ArrowLeft, Database } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { serializeBounds, stationBounds } from '../../lib/geo'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import { SearchableHeader } from '../header/SearchableHeader'
import type { StationSummary } from '../stations/types'
import { fetchDataSourceDetail } from './api'
import { DataSourceMap } from './DataSourceMap'
import { dataSourceImageUrl } from './DataSourcesList'
import { FeatureBadges, ImportStatus, formatSeconds } from './ImportStatus'
import type { DataSourceDetail as DataSourceDetailType } from './types'

/// One metric as its own small bordered box (the overview stats are separate
/// cards so each fact is easy to read at a glance). Compact paddings keep the
/// boxes dense — matching the metric-card language used elsewhere in the app.
function StatCard({
  label,
  children,
  className = '',
}: {
  label: string
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={`flex min-w-0 flex-col gap-1 rounded-lg border bg-card px-3 py-2.5 text-card-foreground shadow-sm ${className}`}
    >
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <span className="min-w-0 text-sm font-semibold leading-snug">{children}</span>
    </div>
  )
}

/// The loading ghost for one stat box. Uses the same grid cell, border and
/// paddings as [`StatCard`] so the loaded cards appear exactly where the ghosts
/// were.
function StatCardSkeleton() {
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border bg-card px-3 py-2.5 shadow-sm">
      <Skeleton className="h-3 w-24" />
      <Skeleton className="h-5 w-16" />
    </div>
  )
}

/// The data-source detail page (`/data-sources/:id`): a large image (logo or the
/// SVG fallback), a map of every provided station, feature badges (including the
/// "not available" ones) and the Data-Overview as separate stat cards.
export function DataSourceDetail() {
  const { dataSourceId } = useParams()
  const navigate = useNavigate()
  const [detail, setDetail] = useState<DataSourceDetailType | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!dataSourceId) return
    let cancelled = false
    setDetail(null)
    setError(false)
    fetchDataSourceDetail(dataSourceId)
      .then((data) => {
        if (!cancelled) setDetail(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
    return () => {
      cancelled = true
    }
  }, [dataSourceId])

  const openDetail = (station: StationSummary) => {
    navigate(`/stations/${station.id}`)
  }

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
          <div className="mb-4 flex items-center justify-between gap-4">
            <Button asChild variant="outline" size="sm">
              <Link to="/data-sources">
                <ArrowLeft /> Back to data sources
              </Link>
            </Button>
            {error && (
              <span className="text-sm font-semibold text-destructive">
                Could not load the data source.
              </span>
            )}
          </div>

          {!error && detail === null && (
            <div aria-busy="true" className="flex flex-col gap-6">
              {/* Row 1: large image + map of the provided stations. */}
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                <Skeleton className="h-64 w-full rounded-lg md:h-80" />
                <Skeleton className="h-64 w-full rounded-lg md:h-80" />
              </div>

              {/* Row 2: name + feature badges. */}
              <div className="flex flex-col gap-2">
                <Skeleton className="h-8 w-64" />
                <Skeleton className="h-4 w-40" />
                <Skeleton className="h-4 w-56" />
                <div className="mt-1 flex flex-wrap gap-2">
                  <Skeleton className="h-6 w-36 rounded-full" />
                  <Skeleton className="h-6 w-44 rounded-full" />
                  <Skeleton className="h-6 w-56 rounded-full" />
                </div>
              </div>

              {/* Data-Overview: the same 2/4 stat-card grid as the loaded view. */}
              <div>
                <Skeleton className="mb-3 h-5 w-44" />
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  {Array.from({ length: 8 }, (_, index) => (
                    <StatCardSkeleton key={index} />
                  ))}
                </div>
              </div>
            </div>
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

function DetailContent({ detail }: { detail: DataSourceDetailType }) {
  const failed = detail.last_import?.status === 'FAILED'
  const lastImport = detail.last_import
  // A run that has not finished has no duration yet: show "Unknown" instead of
  // pretending it ran for a final amount of time.
  const importDuration =
    lastImport && lastImport.duration_seconds !== null
      ? formatSeconds(lastImport.duration_seconds)
      : lastImport
        ? 'Unknown'
        : '—'

  return (
    <div className="flex flex-col gap-6">
      {/* Row 1: large image + map of the provided stations. */}
      <section className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <img
          src={dataSourceImageUrl(detail.image_url)}
          alt={`${detail.name} image`}
          className="h-64 w-full rounded-lg border object-contain p-2 md:h-80"
        />
        <DataSourceMap stations={detail.stations} name={detail.name} />
      </section>

      {/* Row 2: name + feature badges (also the missing capabilities). */}
      <section>
        <div className="flex items-center gap-2">
          <h1 className="flex items-center gap-2 text-2xl font-bold tracking-tight">
            <Database aria-hidden="true" /> {detail.name}
          </h1>
        </div>
        <p className="mt-1 text-sm text-muted-foreground">{detail.provider_type}</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Updated {formatTimestamp(detail.last_updated_at)}
        </p>
        <div className="mt-3">
          <FeatureBadges detail={detail} />
        </div>
      </section>

      {/* A failed last import is surfaced prominently. */}
      {failed && (
        <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
          <AlertTriangle aria-hidden="true" className="mt-0.5 shrink-0" />
          <p>
            The last import failed{detail.last_import?.failure_message ? ':' : '.'}{' '}
            {detail.last_import?.failure_message}
          </p>
        </div>
      )}

      {/* Data-Overview: the current stats as separate compact cards. */}
      <section>
        <h2 className="mb-3 text-lg font-semibold">Data overview</h2>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <StatCard label="Stations">{formatNumber(detail.station_count)}</StatCard>
          <StatCard label="Channels">{formatNumber(detail.channel_count)}</StatCard>
          <StatCard label="First data from">{formatTimestamp(detail.first_data_at)}</StatCard>
          <StatCard label="Last successful import">
            {formatTimestamp(detail.last_updated_at)}
          </StatCard>

          <StatCard label="Last import">
            <ImportStatus import={detail.last_import} />
          </StatCard>
          <StatCard label="Import duration">{importDuration}</StatCard>
          <StatCard label="Warnings (last import)">
            {detail.last_import?.warning_count ?? 0}
          </StatCard>
          <StatCard
            label="Errors (last import)"
            className={(detail.last_import?.error_count ?? 0) > 0 ? 'border-destructive/50' : ''}
          >
            {detail.last_import?.error_count ?? 0}
          </StatCard>
        </div>
      </section>
    </div>
  )
}

export default DataSourceDetail
