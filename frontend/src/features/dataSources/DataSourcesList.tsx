import { useEffect, useState, type ReactNode } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { ArrowLeft, ChevronRight, Database, Info } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { formatNumber, formatTimestamp } from '../../lib/format'
import { serializeBounds, stationBounds } from '../../lib/geo'
import { ErrorBoundary } from '../../lib/ErrorBoundary'
import { SearchableHeader } from '../header/SearchableHeader'
import type { StationSummary } from '../stations/types'
import { fetchDataSources } from './api'
import { ImportStatus } from './ImportStatus'
import type { DataSourceSummary } from './types'
import dataSourceIcon from './data-source.svg'

/// The image a data-source section shows: the provider logo when one is served,
/// else the bundled data-source SVG.
export function dataSourceImageUrl(imageUrl: string): string {
  return imageUrl || dataSourceIcon
}

/// The five columns of the aligned table (desktop). The header row and every
/// body row share this exact template so the columns line up across rows and
/// nothing overflows: name/logo, last-import status, stations, channels, last
/// successful import.
const COLUMNS =
  'md:grid-cols-[minmax(0,1.7fr)_minmax(0,1.2fr)_minmax(0,0.7fr)_minmax(0,0.7fr)_minmax(0,1.4fr)]'

/// One labelled value column inside a list row (mobile stacked layout).
function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <span className="min-w-0 text-sm">{children}</span>
    </div>
  )
}

/// The column header row (desktop only) of the data-source table.
function TableHeader() {
  return (
    <div
      className={`hidden border-b bg-muted/40 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground md:grid ${COLUMNS} gap-x-4`}
    >
      <span>Source</span>
      <span>Last import</span>
      <span>Stations</span>
      <span>Channels</span>
      <span>Last successful import</span>
    </div>
  )
}

/// A compact single row of the table. On `md+` it is one of the five aligned
/// grid columns; on small screens the facts stack under the source name.
function DataSourceRow({ dataSource }: { dataSource: DataSourceSummary }) {
  return (
    <Link
      to={`/data-sources/${dataSource.id}`}
      className="group block transition-colors hover:bg-muted/40"
    >
      {/* Desktop: aligned grid row. */}
      <div className={`hidden items-center gap-x-4 px-4 py-2.5 md:grid ${COLUMNS}`}>
        <div className="flex min-w-0 items-center gap-3">
          <img
            src={dataSourceImageUrl(dataSource.image_url)}
            alt={`${dataSource.name} image`}
            className="h-9 w-9 shrink-0 rounded-md border object-contain p-0.5"
          />
          <div className="min-w-0">
            <div className="truncate font-semibold">{dataSource.name}</div>
            <div className="truncate text-xs text-muted-foreground">{dataSource.provider_type}</div>
          </div>
        </div>

        <div className="min-w-0">
          <ImportStatus import={dataSource.last_import} />
        </div>
        <div className="text-sm tabular-nums">{formatNumber(dataSource.station_count)}</div>
        <div className="text-sm tabular-nums">{formatNumber(dataSource.channel_count)}</div>
        <div className="min-w-0 truncate text-sm">
          {formatTimestamp(dataSource.last_updated_at)}
        </div>
      </div>

      {/* Mobile: name row + a 2-column fact grid. */}
      <div className="flex flex-col gap-2 px-4 py-3 md:hidden">
        <div className="flex items-center gap-3">
          <img
            src={dataSourceImageUrl(dataSource.image_url)}
            alt={`${dataSource.name} image`}
            className="h-10 w-10 shrink-0 rounded-md border object-contain p-0.5"
          />
          <div className="min-w-0 flex-1">
            <div className="truncate font-semibold">{dataSource.name}</div>
            <div className="truncate text-xs text-muted-foreground">{dataSource.provider_type}</div>
          </div>
          <ChevronRight aria-hidden="true" className="h-5 w-5 shrink-0 text-muted-foreground" />
        </div>
        <div className="grid grid-cols-2 gap-x-4 gap-y-2">
          <Fact label="Last import">
            <ImportStatus import={dataSource.last_import} />
          </Fact>
          <Fact label="Last successful import">{formatTimestamp(dataSource.last_updated_at)}</Fact>
          <Fact label="Stations">{formatNumber(dataSource.station_count)}</Fact>
          <Fact label="Channels">{formatNumber(dataSource.channel_count)}</Fact>
        </div>
      </div>
    </Link>
  )
}

/// The loading ghost for the list: the real header plus skeleton rows that use
/// the exact same grid template and paddings, so the table does not jump when
/// the rows load.
function TableSkeleton() {
  return (
    <div aria-busy="true" className="overflow-hidden rounded-lg border bg-card">
      <TableHeader />
      <ul className="divide-y">
        {[0, 1, 2].map((index) => (
          <li key={index} className="md:flex">
            <div className={`hidden items-center gap-x-4 px-4 py-2.5 md:grid ${COLUMNS}`}>
              <div className="flex min-w-0 items-center gap-3">
                <Skeleton className="h-9 w-9 shrink-0 rounded-md" />
                <div className="flex min-w-0 flex-col gap-1.5">
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-3 w-20" />
                </div>
              </div>
              <Skeleton className="h-6 w-24 rounded-full" />
              <Skeleton className="h-4 w-10" />
              <Skeleton className="h-4 w-10" />
              <Skeleton className="h-4 w-28" />
            </div>
            <div className="flex flex-col gap-2 px-4 py-3 md:hidden">
              <div className="flex items-center gap-3">
                <Skeleton className="h-10 w-10 shrink-0 rounded-md" />
                <div className="flex flex-1 flex-col gap-1.5">
                  <Skeleton className="h-4 w-36" />
                  <Skeleton className="h-3 w-24" />
                </div>
              </div>
              <Skeleton className="h-6 w-28 rounded-full" />
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}

/// A short explanation of what the data-sources overview shows and what the
/// status column means.
function DataSourcesInfo() {
  return (
    <div className="mb-4 flex items-start gap-2.5 rounded-lg border bg-muted/40 px-3 py-2.5 text-sm text-muted-foreground">
      <Info aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0" />
      <p>
        A data source is a city&rsquo;s network of bike-counting stations &mdash; for example
        Münster, Bonn or Hamburg. Each row lists one of those sources with the status of its latest
        import and how many stations and channels it provides. Click a row to see a map of its
        stations and more details about the recorded data.
      </p>
    </div>
  )
}

/// The data-sources page (`/data-sources`): one aligned table row per configured
/// data source so name, import status and counts are easy to scan. Clicking a
/// row opens the detail page.
export function DataSourcesList() {
  const navigate = useNavigate()
  const [dataSources, setDataSources] = useState<DataSourceSummary[] | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    fetchDataSources()
      .then((items) => {
        if (!cancelled) setDataSources(items)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
    return () => {
      cancelled = true
    }
  }, [])

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
              <Link to="/">
                <ArrowLeft /> Back to map
              </Link>
            </Button>
            {error && (
              <span className="text-sm font-semibold text-destructive">
                Could not load the data sources.
              </span>
            )}
          </div>

          <header className="mb-3">
            <h1 className="flex items-center gap-2 text-2xl font-bold tracking-tight">
              <Database aria-hidden="true" /> Data sources
            </h1>
          </header>

          <DataSourcesInfo />

          {!error && dataSources !== null && dataSources.length === 0 && (
            <p className="text-sm text-muted-foreground">No data sources configured.</p>
          )}

          {!error && dataSources === null && <TableSkeleton />}

          {!error && dataSources !== null && dataSources.length > 0 && (
            <div className="overflow-hidden rounded-lg border bg-card">
              <TableHeader />
              <ul className="divide-y">
                {dataSources.map((dataSource) => (
                  <li key={dataSource.id}>
                    <DataSourceRow dataSource={dataSource} />
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </main>
    </div>
  )
}

export default function DataSourcesPage() {
  return (
    <ErrorBoundary>
      <DataSourcesList />
    </ErrorBoundary>
  )
}
