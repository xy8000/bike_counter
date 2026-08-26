import { MetricBoxSkeleton, TotalBikesSkeleton } from '../stationDetail/Skeletons'

/// The overview panel's stats loading ghost: the all-time counter card on top,
/// then the four metric boxes stacked in the same single `gap-2` column as the
/// rendered `TotalBikesCard` + `MetricCard` list in `StationOverview`, so the
/// placeholders line up with the boxes that appear once the stats load.
export function OverviewPanelSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <TotalBikesSkeleton />
      <ul className="flex flex-col gap-2">
        {[0, 1, 2, 3].map((index) => (
          <li key={index}>
            <MetricBoxSkeleton />
          </li>
        ))}
      </ul>
    </div>
  )
}
