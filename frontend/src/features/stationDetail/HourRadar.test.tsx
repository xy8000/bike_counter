import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { HourRadar, type HourRadarSeries } from './HourRadar'
import type { HourTotal } from './types'

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

const hour = (hour: number, total: number): HourTotal => ({ hour, total })

describe('HourRadar', () => {
  it('shows the limit notice when there are more than five series', () => {
    const series = Array.from({ length: 6 }, (_, index): HourRadarSeries => ({
      key: `s${index}`,
      label: `Series ${index}`,
      data: [hour(8, 1)],
    }))
    render(<HourRadar series={series} />)
    expect(screen.getByText(/too many data-streams to render/)).toBeInTheDocument()
  })

  it('shows the empty state when there is no traffic at all', () => {
    render(<HourRadar series={[{ key: 'bikes', label: 'Bikes', data: [hour(8, 0)] }]} />)
    expect(screen.getByText('No traffic for this period.')).toBeInTheDocument()
  })

  it('renders a full 24-slot radar with one radar per series', () => {
    const { container } = render(
      <HourRadar
        series={[
          {
            key: 'bikes',
            label: 'Bikes',
            data: [hour(8, 3), hour(20, 12), hour(22, 0)],
          },
        ]}
      />,
    )

    expect(container.querySelector('[data-testid="recharts-RadarChart"]')).not.toBeNull()
    expect(container.querySelectorAll('[data-testid="recharts-Radar"]')).toHaveLength(1)

    const radarChart = container.querySelector('[data-testid="recharts-RadarChart"]')
    const props = JSON.parse(radarChart?.getAttribute('data-recharts-props') ?? '{}') as {
      data: Record<string, string | number>[]
    }
    expect(props.data).toHaveLength(24)
    const morning = props.data.find((row) => row.hour === '08')
    expect(morning?.bikes).toBe(3)
    const empty = props.data.find((row) => row.hour === '22')
    expect(empty?.bikes).toBe(0)
  })

  it('renders one radar per multi-series input', () => {
    const { container } = render(
      <HourRadar
        series={[
          { key: 'current', label: 'Current', data: [hour(8, 3)] },
          { key: 'previous', label: 'Previous', data: [hour(8, 1)] },
        ]}
      />,
    )
    expect(container.querySelectorAll('[data-testid="recharts-Radar"]')).toHaveLength(2)
  })
})
