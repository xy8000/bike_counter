import { formatNumber } from '../../lib/format'

/// The all-time bike counter shared by the overview panel, the detail page and
/// the station-summary page: a highlighted card showing the lifetime total.
/// There is no trend because the whole history has no comparison period.
export function TotalBikesCard({ total }: { total: number }) {
  return (
    <div className="rounded-md border bg-muted/40 p-4">
      <p className="text-sm font-medium text-muted-foreground">
        Total bikes (all time)
      </p>
      <p className="mt-1 text-3xl font-semibold leading-tight">
        {formatNumber(total)}
        <span className="ml-1 text-xs font-normal text-muted-foreground">bikes</span>
      </p>
    </div>
  )
}
