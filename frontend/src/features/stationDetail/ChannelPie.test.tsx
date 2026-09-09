import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { ChannelPie } from './ChannelPie'
import type { ChannelRef, ChannelTotal } from './types'

// The shared recharts mock eagerly JSON.stringify's every non-children prop.
// The shadcn ChartTooltip alias forwards a `content` React element (whose fiber
// owner is circular), which would crash that serialization. Keep the real
// ChartContainer/ChartTooltipContent but null the Tooltip sink so the element
// is never serialized.
vi.mock('@/components/ui/chart', async () => {
  const actual =
    await vi.importActual<typeof import('@/components/ui/chart')>('@/components/ui/chart')
  return { ...actual, ChartTooltip: () => null, ChartLegend: () => null }
})

const total = (channel_id: string, value: number): ChannelTotal => ({ channel_id, total: value })

describe('ChannelPie', () => {
  it('maps channel ids to their names in the legend', () => {
    const channels: ChannelRef[] = [
      { id: 'c1', name: 'North' },
      { id: 'c2', name: 'South' },
    ]
    render(<ChannelPie totals={[total('c1', 30), total('c2', 20)]} channels={channels} />)

    expect(screen.getByText('North · 30')).toBeInTheDocument()
    expect(screen.getByText('South · 20')).toBeInTheDocument()
  })

  it('falls back to the channel id when no matching channel name exists', () => {
    render(<ChannelPie totals={[total('ghost', 12)]} channels={[{ id: 'c1', name: 'North' }]} />)

    expect(screen.getByText('ghost · 12')).toBeInTheDocument()
  })

  it('delegates the empty state to SharePie when there is no traffic', () => {
    render(<ChannelPie totals={[total('c1', 0)]} channels={[{ id: 'c1', name: 'North' }]} />)
    expect(screen.getByText('No traffic for this period.')).toBeInTheDocument()
  })
})
