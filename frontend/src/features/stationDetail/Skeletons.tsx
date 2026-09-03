import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'

/// A page-shell loading placeholder (hero image + a couple of text lines),
/// shown while the detail/summary shell sub-request is in flight.
export function PageShellSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <Skeleton className="h-64 w-full rounded-lg md:h-80" />
      <Skeleton className="h-8 w-1/2 max-w-72" />
      <Skeleton className="h-4 w-2/3 max-w-96" />
    </div>
  )
}

/// One metric-box skeleton, sized like `MetricCard`.
export function MetricBoxSkeleton() {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border p-3">
      <div className="min-w-0 flex-1">
        <Skeleton className="h-4 w-24" />
        <Skeleton className="mt-2 h-7 w-20" />
      </div>
      <div className="flex shrink-0 flex-col items-end gap-0.5">
        <Skeleton className="h-4 w-14" />
        <Skeleton className="h-3 w-16" />
      </div>
    </div>
  )
}

/// The all-time counter skeleton, sized like `TotalBikesCard`: the same
/// bordered `bg-muted/40 p-4` box with a label line and a value line inside.
export function TotalBikesSkeleton() {
  return (
    <div className="rounded-md border bg-muted/40 p-4">
      <Skeleton className="h-4 w-36" />
      <Skeleton className="mt-2 h-9 w-28" />
    </div>
  )
}

/// The overview card skeleton: the all-time counter on top + a grid of metric
/// boxes, matching the rendered `TotalBikesCard` + `MetricCard` grid.
export function OverviewSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-24 w-full rounded-md" />
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {[0, 1, 2, 3].map((index) => (
          <MetricBoxSkeleton key={index} />
        ))}
      </div>
    </div>
  )
}

/// One chart-card skeleton with a header line + a chart-shaped block.
function ChartCardSkeleton() {
  return (
    <Card className="gap-2">
      <CardHeader className="px-4 pb-1 pt-4">
        <Skeleton className="h-5 w-36" />
      </CardHeader>
      <CardContent className="px-4 pb-4">
        <Skeleton className="aspect-[21/9] w-full" />
      </CardContent>
    </Card>
  )
}

/// The "Detailed statistics" / "Detailed stats" skeleton: a full-width bar-chart
/// card + two radar cards, matching the rendered chart grid.
export function ChartsSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <ChartCardSkeleton />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <ChartCardSkeleton />
        <ChartCardSkeleton />
      </div>
    </div>
  )
}

/// The key-facts row skeleton: four bordered boxes (label + large value +
/// detail line) matching the rendered `KeyFacts` grid, shown while the graphs
/// card that the facts are derived from is still loading.
export function KeyFactsSkeleton() {
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
      {[0, 1, 2, 3].map((index) => (
        <div key={index} className="rounded-md border p-3">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="mt-2 h-7 w-20" />
          <Skeleton className="mt-2 h-3 w-16" />
        </div>
      ))}
    </div>
  )
}

/// The monthly bar chart skeleton: a card with a year-button row (matching the
/// interactive year selector) + a bar-shaped block, matching `MonthlyBarChart`.
export function MonthlyBarSkeleton() {
  return (
    <Card className="py-0">
      <CardHeader className="flex flex-wrap items-stretch border-b p-0">
        {[0, 1].map((index) => (
          <div
            key={index}
            className="flex flex-1 flex-col justify-center gap-1 px-4 py-3 sm:px-6 sm:py-4"
          >
            <Skeleton className="h-3 w-10" />
            <Skeleton className="h-6 w-20" />
            <Skeleton className="h-3 w-12" />
          </div>
        ))}
      </CardHeader>
      <CardContent className="flex flex-col gap-2 px-6 py-4">
        <Skeleton className="h-40 w-full" />
      </CardContent>
    </Card>
  )
}
