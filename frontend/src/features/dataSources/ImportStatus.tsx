import { useEffect, useState } from 'react'
import { CircleCheck, CircleX, Loader2, X } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import type { DataSourceDetail, DataSourceImport } from './types'

/// Formats a whole-second duration (`3m 12s`, `1h 5m`, …).
export function formatSeconds(total: number): string {
  const seconds = Math.max(0, Math.floor(total))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

/// The elapsed time of a RUNNING import, ticking every second.
export function RunningDuration({ startedAt }: { startedAt: string }) {
  const start = Date.parse(startedAt)
  const tick = () => Math.floor((Date.now() - start) / 1000)
  const [elapsed, setElapsed] = useState(tick)
  useEffect(() => {
    const interval = window.setInterval(() => setElapsed(tick()), 1000)
    return () => window.clearInterval(interval)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startedAt])
  return <span>{formatSeconds(elapsed)}</span>
}

/// The import status with a distinguishing icon, used in the list and the
/// detail. A RUNNING import additionally shows how long it has been running.
export function ImportStatus({ import: run }: { import: DataSourceImport | null }) {
  if (run === null) {
    return <Badge variant="outline">Never imported</Badge>
  }
  if (run.status === 'RUNNING') {
    return (
      <Badge variant="secondary">
        <Loader2 aria-hidden="true" className="h-3 w-3 animate-spin" />
        Running · <RunningDuration startedAt={run.started_at} />
      </Badge>
    )
  }
  if (run.status === 'FAILED') {
    return (
      <Badge variant="destructive">
        <CircleX aria-hidden="true" className="h-3 w-3" /> Failed
      </Badge>
    )
  }
  return (
    <Badge variant="outline" className="text-emerald-600 dark:text-emerald-400">
      <CircleCheck aria-hidden="true" className="h-3 w-3" /> Succeeded
    </Badge>
  )
}

/// The three feature badges, showing both what the source offers and what it
/// lacks (so a missing capability is visible instead of just absent).
export function FeatureBadges({ detail }: { detail: DataSourceDetail }) {
  const badges: { label: string; available: boolean }[] = [
    { label: 'Historical data', available: detail.has_historical },
    { label: 'Real-time data', available: detail.has_real_time },
    { label: 'Full current year coverage', available: detail.has_full_current_year },
  ]
  return (
    <div className="flex flex-wrap items-center gap-2">
      {badges.map(({ label, available }) =>
        available ? (
          <Badge key={label}>
            <CircleCheck aria-hidden="true" className="h-3 w-3" /> {label}
          </Badge>
        ) : (
          <Badge
            key={label}
            variant="outline"
            className="text-muted-foreground"
            title={`Not available for this data source`}
          >
            <X aria-hidden="true" className="h-3 w-3" /> No {label.toLowerCase()}
          </Badge>
        ),
      )}
    </div>
  )
}
