import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { MonthlyBarChart } from './MonthlyBarChart'
import type { MonthTotal } from './types'

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

const entry = (year: number, month: number, total: number): MonthTotal => ({
  year,
  month,
  total,
})

function yearButton(name: string | RegExp) {
  const buttons = screen.getAllByRole('button')
  return buttons.find((button) => button.textContent?.includes(String(name)))
}

describe('MonthlyBarChart', () => {
  it('shows the empty state when there are no monthly totals', () => {
    render(<MonthlyBarChart totals={[]} />)
    expect(screen.getByText('No monthly data yet.')).toBeInTheDocument()
  })

  it('renders one year button per year with its total and trend', () => {
    const totals = [entry(2023, 1, 100), entry(2024, 1, 120), entry(2025, 1, 150)]
    render(<MonthlyBarChart totals={totals} />)

    expect(screen.getAllByRole('button')).toHaveLength(3)
    // The most recent year is selected by default.
    expect(yearButton('2025')).toHaveAttribute('data-active', 'true')
    expect(yearButton('2024')).toHaveAttribute('data-active', 'false')

    // 2024 gained 20% over 2023 (up); the latest year shows a dash.
    expect(screen.getByText('+20%')).toBeInTheDocument()
    expect(screen.getByLabelText('up')).toBeInTheDocument()
    expect(screen.getAllByText('–')).toHaveLength(2)
  })

  it('draws twelve monthly bars for the selected year', () => {
    const totals = [entry(2023, 1, 100), entry(2024, 1, 120), entry(2024, 12, 40)]
    const { container } = render(<MonthlyBarChart totals={totals} />)

    const barChart = container.querySelector('[data-testid="recharts-BarChart"]')
    expect(barChart).not.toBeNull()
    const props = JSON.parse(barChart?.getAttribute('data-recharts-props') ?? '{}') as {
      data: Record<string, string | number>[]
    }
    expect(props.data).toHaveLength(12)
    expect(props.data[0].month).toBe('Jan')
    expect(props.data[0]['2023']).toBe(100)
    expect(props.data[0]['2024']).toBe(120)
    expect(props.data[11]['2024']).toBe(40)
  })

  it('switches the active year when a year button is clicked', () => {
    const totals = [entry(2023, 1, 100), entry(2024, 1, 120)]
    const { container } = render(<MonthlyBarChart totals={totals} />)

    const bar = container.querySelector('[data-testid="recharts-Bar"]')
    expect(JSON.parse(bar?.getAttribute('data-recharts-props') ?? '{}')).toMatchObject({
      dataKey: '2024',
    })

    fireEvent.click(yearButton('2023') as HTMLElement)

    expect(yearButton('2023')).toHaveAttribute('data-active', 'true')
    expect(yearButton('2024')).toHaveAttribute('data-active', 'false')
    const activeBar = container.querySelector('[data-testid="recharts-Bar"]')
    expect(JSON.parse(activeBar?.getAttribute('data-recharts-props') ?? '{}')).toMatchObject({
      dataKey: '2023',
    })
  })
})
