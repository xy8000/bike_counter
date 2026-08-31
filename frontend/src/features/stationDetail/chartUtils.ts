/// Shared chart helpers for the detail page's graphs.

/// The five CSS chart tokens (light/dark aware) cycled for series.
export const CHART_PALETTE = [
  'var(--color-chart-1)',
  'var(--color-chart-2)',
  'var(--color-chart-3)',
  'var(--color-chart-4)',
  'var(--color-chart-5)',
]

/// The maximum number of data-streams (stations / channels) a chart is allowed
/// to render. Charts that would draw more are replaced by the info note.
export const MAX_DATA_STREAMS = 5

/// Colour for the n-th series. The palette holds one token per allowed stream
/// (charts with more than `MAX_DATA_STREAMS` are hidden), so the modulo only
/// guards against callers ignoring the limit.
export function seriesColor(index: number): string {
  return CHART_PALETTE[index % CHART_PALETTE.length]
}
