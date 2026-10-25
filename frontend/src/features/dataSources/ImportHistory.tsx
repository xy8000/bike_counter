import { useEffect, useState } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { formatTimestamp } from '../../lib/format'
import { fetchDataSourceImports } from './api'
import { formatSeconds } from './ImportStatus'
import type { DataSourceImport, DataSourceImports } from './types'

/// The import-run history table of a data source plus the average run duration
/// (calculated from the finished runs). Each run is one row: start time,
/// duration, success, warnings, errors, new measurements and new stations.
export function ImportHistory({ dataSourceId }: Readonly<{ dataSourceId: string }>) {
  const [imports, setImports] = useState<DataSourceImports | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    setImports(null)
    setError(false)
    fetchDataSourceImports(dataSourceId)
      .then((data) => {
        if (!cancelled) setImports(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
    return () => {
      cancelled = true
    }
  }, [dataSourceId])

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
      )}
    </section>
  )
}

function RunRow({ run }: Readonly<{ run: DataSourceImport }>) {
  const duration = run.duration_seconds !== null ? formatSeconds(run.duration_seconds) : 'Unknown'
  return (
    <tr className="border-b last:border-b-0">
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
