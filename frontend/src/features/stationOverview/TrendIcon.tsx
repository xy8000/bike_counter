import { Minus, TrendingDown, TrendingUp } from 'lucide-react'
import type { Trend } from './types'

/// The neat up/down/flat arrow for a metric trend.
export function TrendIcon({ trend }: { trend: Trend }) {
  if (trend === 'up') {
    return <TrendingUp className="h-4 w-4 text-emerald-600" aria-label="up" />
  }
  if (trend === 'down') {
    return <TrendingDown className="h-4 w-4 text-rose-600" aria-label="down" />
  }
  return <Minus className="h-4 w-4 text-muted-foreground" aria-label="flat" />
}
