/// Shared chart helpers for the detail page's graphs.

/// The five CSS chart tokens (light/dark aware) cycled for series.
export const CHART_PALETTE = [
  'var(--color-chart-1)',
  'var(--color-chart-2)',
  'var(--color-chart-3)',
  'var(--color-chart-4)',
  'var(--color-chart-5)',
]

/// Colour for the n-th series; wraps around when a station has more than five
/// channels.
export function seriesColor(index: number): string {
  return CHART_PALETTE[index % CHART_PALETTE.length]
}
