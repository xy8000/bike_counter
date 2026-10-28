import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { formatTimestamp } from '../../lib/format'
import { fetchDataSourceImports, IMPORT_PAGE_SIZE } from './api'
import { formatSeconds } from './ImportStatus'
import type { DataSourceImport, DataSourceImports } from './types'

/// The paginated import-run history table of a data source plus the average run
/// duration (calculated server-side over all finished runs). Each run is one
/// row: start time, duration, success, warnings, errors, new measurements and
/// new stations. The newest 50 runs are shown by default; Previous/Next page
/// through the rest.
export function ImportHistory({ dataSourceId }: Readonly<{ dataSourceId: string }>) {
  const [imports, setImports] = useState<DataSourceImports | null>(null)
  const [error, setError] = useState(false)
  const [offset, setOffset] = useState(0)

  // Restart at the first page when the data source changes without remounting.
  // Adjusting the state during render avoids a fetch with the previous source's
  // offset, which would otherwise show the wrong page for a moment.
  const [pageSourceId, setPageSourceId] = useState(dataSourceId)
  if (pageSourceId !== dataSourceId) {
    setPageSourceId(dataSourceId)
    setOffset(0)
  }

  useEffect(() => {
    let cancelled = false
    setImports(null)
    setError(false)
    fetchDataSourceImports(dataSourceId, offset, IMPORT_PAGE_SIZE)
      .then((data) => {
        if (!cancelled) setImports(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
    return () => {
      cancelled = true
    }
  }, [dataSourceId, offset])

  return (
    <section aria-label="Import history">
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-lg font-semibold">Import jobs</h2>
        {imports?.average_duration_seconds != null && (
          <p className="text-sm text-muted-foreground">
            Average import duration:{' '}
            <span className="font-medium text-foreground">
              {formatSeconds(imports.average_duration_seconds)}
            </span>
          </p>
        )}
      </div>

      {!error && imports === null && (
        <div aria-busy="true" className="flex flex-col gap-2">
          <Skeleton className="h-5 w-full" />
          <Skeleton className="h-5 w-full" />
          <Skeleton className="h-5 w-3/4" />
        </div>
      )}

      {error && <p className="text-sm text-muted-foreground">Could not load the import jobs.</p>}

      {!error && imports?.items.length === 0 && (
        <p className="text-sm text-muted-foreground">No import jobs recorded yet.</p>
      )}

      {!error && imports && imports.items.length > 0 && (
        <>
          <div className="overflow-x-auto rounded-lg border">
            <table className="w-full min-w-[56rem] border-collapse text-sm">
              <thead>
                <tr className="border-b bg-muted/50 text-left text-xs font-medium text-muted-foreground">
                  <th className="px-3 py-2 font-medium">Start time</th>
                  <th className="px-3 py-2 font-medium">Duration</th>
                  <th className="px-3 py-2 font-medium">Successful</th>
                  <th className="px-3 py-2 text-right font-medium">Warnings</th>
                  <th className="px-3 py-2 text-right font-medium">Errors</th>
                  <th className="px-3 py-2 text-right font-medium">New measurements</th>
                  <th className="px-3 py-2 text-right font-medium">New stations</th>
                </tr>
              </thead>
              <tbody>
                {imports.items.map((run) => (
                  <RunRow key={`${run.started_at}-${run.status}`} run={run} />
                ))}
              </tbody>
            </table>
          </div>
          <ImportPagination imports={imports} onOffsetChange={setOffset} />
        </>
      )}
    </section>
  )
}

/// The Previous/Next footer of the import-run history.
function ImportPagination({
  imports,
  onOffsetChange,
}: Readonly<{ imports: DataSourceImports; onOffsetChange: (offset: number) => void }>) {
  const pageSize = Math.max(1, imports.limit)
  const start = imports.offset + 1
  const end = imports.offset + imports.items.length
  const page = Math.floor(imports.offset / pageSize) + 1
  const pageCount = Math.max(1, Math.ceil(imports.total / pageSize))

  return (
    <nav
      aria-label="Import history pagination"
      className="mt-3 flex flex-wrap items-center justify-between gap-3 text-sm text-muted-foreground"
    >
      <span>
        Showing {start}-{end} of {imports.total} import jobs
      </span>
      <span>
        Page {page} of {pageCount}
      </span>
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onOffsetChange(Math.max(0, imports.offset - pageSize))}
          disabled={imports.offset === 0}
        >
          Previous
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onOffsetChange(imports.offset + pageSize)}
          disabled={end >= imports.total}
        >
          Next
        </Button>
      </div>
    </nav>
  )
}

function RunRow({ run }: Readonly<{ run: DataSourceImport }>) {
  const duration = run.duration_seconds !== null ? formatSeconds(run.duration_seconds) : 'Unknown'
  return (
    <tr data-testid="import-run-row" className="border-b last:border-b-0">
      <td className="px-3 py-2 whitespace-nowrap">{formatTimestamp(run.started_at)}</td>
      <td className="px-3 py-2 whitespace-nowrap">{duration}</td>
      <td className="px-3 py-2 whitespace-nowrap">{run.successful ? 'Yes' : 'No'}</td>
      <td className="px-3 py-2 text-right tabular-nums">{run.warning_count}</td>
      <td className="px-3 py-2 text-right tabular-nums">{run.error_count}</td>
      <td className="px-3 py-2 text-right tabular-nums">{run.added_measurements}</td>
      <td className="px-3 py-2 text-right tabular-nums">{run.added_stations}</td>
    </tr>
  )
}
