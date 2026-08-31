import { cn } from '@/lib/utils'

/// Month labels under the chart (Jan–Aug).
const MONTHS = ['J', 'F', 'M', 'A', 'M', 'J', 'J', 'A']

/// The growth from the stations that have a full year of data (percent of the
/// tallest bar). The single chart switches between this and the "all stations"
/// version below.
const BASE = [12, 18, 26, 34, 42, 55, 66, 74]

/// Extra bikes a newly-built station contributes once it opens (month 3
/// onwards). Adding it to the same data produces the visible jumps.
const NEW_STATION = [0, 0, 28, 30, 46, 40, 32, 26]

/// Decorative, accessible-hidden illustration for the "exclude new stations"
/// setting. It draws one monthly chart and SWITCHES between the two states when
/// the setting changes:
/// - off: all stations, with a new station's bikes stacked on top (jumps);
/// - on:  established only, the same base data without that station (even growth).
export function TrendSettingsIllustration({ excludeNewStations }: { excludeNewStations: boolean }) {
  const includeNewStation = !excludeNewStations

  return (
    <div
      className={cn(
        'rounded-lg border p-3',
        excludeNewStations && 'border-primary/40 bg-primary/5',
      )}
      aria-hidden="true"
    >
      <p className="mb-2 text-center text-xs font-semibold text-foreground">
        {excludeNewStations ? 'Established only' : 'All stations'}
      </p>
      <div className="flex h-32 items-end justify-center gap-1.5">
        {BASE.map((base, index) => {
          const added = includeNewStation ? NEW_STATION[index] : 0
          const hasNew = added > 0
          return (
            <div key={index} className="flex h-full w-full max-w-6 flex-col justify-end">
              {hasNew && (
                <div
                  className="w-full rounded-t-sm bg-primary transition-all duration-200"
                  style={{ height: `${added}%` }}
                />
              )}
              <div
                className={cn(
                  'w-full bg-primary/45 transition-all duration-200',
                  hasNew ? 'rounded-b-sm' : 'rounded-sm',
                )}
                style={{ height: `${base}%` }}
              />
            </div>
          )
        })}
      </div>
      <div className="mt-1 flex justify-between text-[8px] leading-none text-muted-foreground">
        <span>{MONTHS[0]}</span>
        <span>{MONTHS[MONTHS.length - 1]}</span>
      </div>
      {includeNewStation ? (
        <div className="mt-2 flex items-center justify-center gap-3 text-[10px] text-muted-foreground">
          <span className="flex items-center gap-1">
            <span className="h-2 w-2 rounded-sm bg-primary/45" />
            existing stations
          </span>
          <span className="flex items-center gap-1">
            <span className="h-2 w-2 rounded-sm bg-primary" />
            new station
          </span>
        </div>
      ) : (
        <p className="mt-2 text-center text-[10px] text-muted-foreground">
          only stations with a full year of data
        </p>
      )}
      <p className="mt-1 text-center text-[11px] leading-snug text-muted-foreground">
        {excludeNewStations
          ? 'new station excluded — totals grow more evenly'
          : 'a new station opens and adds bikes — recent months jump'}
      </p>
    </div>
  )
}
