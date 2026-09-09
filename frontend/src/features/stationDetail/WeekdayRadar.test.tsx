import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { WeekdayRadar, type RadarSeries } from './WeekdayRadar'
import type { WeekdayTotal } from './types'

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

const day = (weekday: number, total: number): WeekdayTotal => ({ weekday, total })

describe('WeekdayRadar', () => {
  it('shows the limit notice when there are more than five series', () => {
    const series = Array.from({ length: 6 }, (_, index): RadarSeries => ({
      key: `s${index}`,
      label: `Series ${index}`,
      data: [day(1, 1)],
    }))
    render(<WeekdayRadar series={series} />)
    expect(screen.getByText(/too many data-streams to render/)).toBeInTheDocument()
  })

  it('shows the empty state when there is no traffic at all', () => {
    render(<WeekdayRadar series={[{ key: 'bikes', label: 'Bikes', data: [day(1, 0)] }]} />)
    expect(screen.getByText('No traffic for this period.')).toBeInTheDocument()
  })

  it('renders a 7-slot radar with one radar per series', () => {
    const { container } = render(
      <WeekdayRadar series={[{ key: 'bikes', label: 'Bikes', data: [day(1, 10), day(3, 5)] }]} />,
    )

    expect(container.querySelector('[data-testid="recharts-RadarChart"]')).not.toBeNull()
    expect(container.querySelectorAll('[data-testid="recharts-Radar"]')).toHaveLength(1)

    const radarChart = container.querySelector('[data-testid="recharts-RadarChart"]')
    const props = JSON.parse(radarChart?.getAttribute('data-recharts-props') ?? '{}') as {
      data: Record<string, string | number>[]
    }
    expect(props.data).toHaveLength(7)
    // Index 0 is Monday (weekday 1).
    expect(props.data[0].bikes).toBe(10)
    // Missing/unspecified weekdays fall back to 0 (index 1 = Tuesday).
    expect(props.data[1].bikes).toBe(0)
    expect(props.data[2].bikes).toBe(5)
  })
})
