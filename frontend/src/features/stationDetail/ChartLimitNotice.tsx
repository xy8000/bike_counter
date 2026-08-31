import { cn } from '@/lib/utils'
import { MAX_DATA_STREAMS } from './chartUtils'

/// Info note shown instead of a chart when a chart would render more than
/// `MAX_DATA_STREAMS` data-streams (stations / channels). Keeps the same
/// centered muted-text look as `ChartEmptyState` but with "too many" wording.
export function ChartLimitNotice({ message, className }: { message?: string; className?: string }) {
  return (
    <div className={cn('flex items-center justify-center', className)}>
      <p className="text-sm text-muted-foreground">
        {message ??
          `This chart cannot be loaded — too many data-streams to render (max ${MAX_DATA_STREAMS}).`}
      </p>
    </div>
  )
}
