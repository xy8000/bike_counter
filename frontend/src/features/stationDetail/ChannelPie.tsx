import type { ChannelRef, ChannelTotal } from './types'
import { SharePie, type ShareSlice } from './SharePie'

/// Donut of each channel's share over the selected timeframe (detail page).
/// Thin wrapper over the shared [`SharePie`] that maps channel totals + names
/// into generic slices; the summary page feeds it station slices instead.
export function ChannelPie({
  totals,
  channels,
  className,
}: {
  totals: ChannelTotal[]
  channels: ChannelRef[]
  className?: string
}) {
  const nameOf = (id: string) => channels.find((channel) => channel.id === id)?.name ?? id
  const slices: ShareSlice[] = totals.map((total) => ({
    id: total.channel_id,
    name: nameOf(total.channel_id),
    total: total.total,
  }))
  return <SharePie slices={slices} className={className} />
}
