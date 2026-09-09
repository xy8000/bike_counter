import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { TimeSeriesBarChart, type BarSeries } from './TimeSeriesBarChart'
import type { TimeBucket } from './types'

// The shared recharts mock eagerly JSON.stringify's every non-children prop.
// The shadcn ChartTooltip alias forwards a `content` React element (whose fiber
// owner is circular), which would crash that serialization. Keep the real
// ChartContainer/ChartTooltipContent but null the Tooltip/Legend sinks so the
// elements are never serialized.
vi.mock('@/components/ui/chart', async () => {
  const actual =
    await vi.importActual<typeof import('@/components/ui/chart')>('@/components/ui/chart')
  return { ...actual, ChartTooltip: () => null, ChartLegend: () => null }
})

const bucket = (start: string, total: number): TimeBucket => ({ start, total })
const series = (key: string, label: string, data: TimeBucket[]): BarSeries => ({
  key,
  label,
  stackId: 'current',
  data,
})

function barChartProps(container: HTMLElement) {
  const node = container.querySelector('[data-testid="recharts-BarChart"]')
  expect(node).not.toBeNull()
  return JSON.parse(node?.getAttribute('data-recharts-props') ?? '{}') as {
    data: { time: number; [key: string]: number | null | undefined }[]
  }
}

describe('TimeSeriesBarChart', () => {
  it('shows the empty state when every series has no buckets', () => {
    render(<TimeSeriesBarChart series={[series('a', 'A', [])]} xFormatter={() => ''} />)
    expect(screen.getByText('No data for this period.')).toBeInTheDocument()
  })

  it('shows the limit notice when more than five series have data', () => {
    const many = Array.from({ length: 6 }, (_, index) =>
      series(`s${index}`, `S${index}`, [bucket('2024-01-01T10:00:00.000Z', index)]),
    )
    render(<TimeSeriesBarChart series={many} xFormatter={() => ''} />)
    expect(screen.getByText(/too many data-streams to render/)).toBeInTheDocument()
  })

  it('drops empty series before the limit check', () => {
    const many = Array.from({ length: 6 }, (_, index) =>
      index === 5
        ? series('empty', 'Empty', [])
        : series(`s${index}`, `S${index}`, [bucket('2024-01-01T10:00:00.000Z', index)]),
    )
    const { container } = render(<TimeSeriesBarChart series={many} xFormatter={() => ''} />)
    // Five visible series pass the limit and render one bar each.
    expect(container.querySelectorAll('[data-testid="recharts-Bar"]')).toHaveLength(5)
  })

  it('merges series buckets into one point per timestamp, sorted by time', () => {
    const { container } = render(
      <TimeSeriesBarChart
        series={[
          series('current', 'Current', [
            bucket('2024-01-01T12:00:00.000Z', 7),
            bucket('2024-01-01T10:00:00.000Z', 5),
          ]),
          series('previous', 'Previous', [
            bucket('2024-01-01T10:00:00.000Z', 2),
            bucket('2024-01-01T11:00:00.000Z', 3),
          ]),
        ]}
        xFormatter={() => ''}
      />,
    )

    const data = barChartProps(container).data
    expect(data).toHaveLength(3)
    const times = data.map((point) => point.time)
    expect([...times].sort((a, b) => a - b)).toEqual(times)

    const at10 = data.find((point) => point.time === new Date('2024-01-01T10:00:00.000Z').getTime())
    expect(at10?.current).toBe(5)
    expect(at10?.previous).toBe(2)
    const at11 = data.find((point) => point.time === new Date('2024-01-01T11:00:00.000Z').getTime())
    expect(at11?.previous).toBe(3)
  })

  it('renders one bar per visible series', () => {
    const { container } = render(
      <TimeSeriesBarChart
        series={[
          series('current', 'Current', [bucket('2024-01-01T10:00:00.000Z', 5)]),
          series('previous', 'Previous', [bucket('2024-01-01T11:00:00.000Z', 2)]),
        ]}
        xFormatter={() => ''}
      />,
    )
    expect(container.querySelectorAll('[data-testid="recharts-Bar"]')).toHaveLength(2)
  })
})
