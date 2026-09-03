import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { formatNumber, formatTimestamp } from '../../lib/format'
import type { GlobalSummary } from './types'

/// Popup dialogue for the whole-system summary in the header. Only the
/// `updated …` timestamp stays visible in the top bar; clicking it opens this
/// dialogue with the counting stations, channels and "bikes / last day" totals
/// (and repeats the update timestamp so nothing is hidden).
export function GlobalSummaryDialog({
  summary,
  onClose,
}: {
  summary: GlobalSummary
  onClose: () => void
}) {
  const facts: Array<{ label: string; value: string }> = [
    { label: 'Counting stations', value: String(summary.station_count) },
    { label: 'Channels', value: formatNumber(summary.channel_count) },
    { label: 'Bikes / last day', value: formatNumber(summary.bikes_last_day_total) },
    { label: 'Updated', value: formatTimestamp(summary.last_update) },
  ]

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Global summary</DialogTitle>
          <DialogDescription>Whole-system counting statistics.</DialogDescription>
        </DialogHeader>
        <dl className="flex flex-col gap-3 text-sm">
          {facts.map((fact) => (
            <div key={fact.label} className="flex items-baseline justify-between gap-4">
              <dt className="text-xs font-medium text-muted-foreground">{fact.label}</dt>
              <dd className="font-medium">{fact.value}</dd>
            </div>
          ))}
        </dl>
      </DialogContent>
    </Dialog>
  )
}
