import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { SharePie, type ShareSlice } from './SharePie'

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

const slice = (id: string, name: string, total: number): ShareSlice => ({ id, name, total })

describe('SharePie', () => {
  it('shows the empty state when every slice has no traffic', () => {
    render(<SharePie slices={[slice('a', 'North', 0), slice('b', 'South', 0)]} />)
    expect(screen.getByText('No traffic for this period.')).toBeInTheDocument()
    expect(screen.queryByText(/·/)).not.toBeInTheDocument()
  })

  it('shows the limit notice when more than five slices have traffic', () => {
    const slices = Array.from({ length: 6 }, (_, index) =>
      slice(`c${index}`, `Channel ${index}`, index + 1),
    )
    render(<SharePie slices={slices} />)
    expect(screen.getByText(/too many data-streams to render/)).toBeInTheDocument()
  })

  it('renders the donut and a legend entry per slice with traffic', () => {
    const slices = [slice('c1', 'North', 30), slice('c2', 'South', 20), slice('c3', 'Empty', 0)]
    const { container } = render(<SharePie slices={slices} />)

    expect(container.querySelector('[data-testid="recharts-PieChart"]')).not.toBeNull()
    // Only the two slices with traffic get a segment cell.
    expect(container.querySelectorAll('[data-testid="recharts-Cell"]')).toHaveLength(2)

    expect(screen.getByText('North · 30')).toBeInTheDocument()
    expect(screen.getByText('South · 20')).toBeInTheDocument()
    expect(screen.queryByText('Empty · 0')).not.toBeInTheDocument()
  })
})
