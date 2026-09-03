import { cn } from '@/lib/utils'

/// Shared empty state for the station-detail charts. Keeps the look and wording
/// consistent across the bar chart, the donut and the radar when a period has no
/// traffic yet, so every chart degrades the same way.
export function ChartEmptyState({
  message = 'No data for this period.',
  className,
}: {
  message?: string
  className?: string
}) {
  return (
    <div className={cn('flex items-center justify-center', className)}>
      <p className="text-sm text-muted-foreground">{message}</p>
    </div>
  )
}
