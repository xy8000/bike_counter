import { render, screen } from '@testing-library/react'
import type { ComponentProps, ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartStyle,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from './chart'

// The chart wrappers hand a React element (`content`) to the recharts Tooltip /
// Legend. The shared setup mock JSON.stringify's props, which throws on the
// circular element structures, so this file replaces it with a version that
// only renders a labelled wrapper (no prop stringification). ChartTooltipContent
// / ChartLegendContent are pure and are tested directly with crafted props.
vi.mock('recharts', async () => {
  const React = (await import('react')).default
  const make = (name: string) => {
    const Comp = (props: { children?: ReactNode }) =>
      React.createElement('div', { 'data-testid': `recharts-${name}` }, props.children)
    Comp.displayName = name
    return Comp
  }
  return {
    ResponsiveContainer: make('ResponsiveContainer'),
    Tooltip: make('Tooltip'),
    Legend: make('Legend'),
  }
})

type TooltipPayload = ComponentProps<typeof ChartTooltipContent>['payload']
type LegendPayload = ComponentProps<typeof ChartLegendContent>['payload']

const baseConfig = {
  desktop: { label: 'Desktop', color: '#2563eb' },
  mobile: { label: 'Mobile', color: '#f97316' },
} satisfies ChartConfig

function entry(overrides: Record<string, unknown> = {}) {
  return {
    name: 'desktop',
    dataKey: 'desktop',
    value: 42,
    color: '#2563eb',
    // Real recharts payload items always carry the source datum; chart.tsx
    // reads `item.payload.fill` for the indicator colour.
    payload: { fill: '#2563eb' },
    ...overrides,
  }
}

function tooltipPayload(...items: Array<Record<string, unknown>>): TooltipPayload {
  return items as unknown as TooltipPayload
}

function legendPayload(...items: Array<Record<string, unknown>>): LegendPayload {
  return items as unknown as LegendPayload
}

function renderInContainer(config: ChartConfig, content: ReactNode) {
  return render(<ChartContainer config={config}>{content}</ChartContainer>)
}

describe('ChartStyle', () => {
  it('injects a style with CSS variables for colored config entries', () => {
    const { container } = render(<ChartStyle id="rev" config={baseConfig} />)
    const style = container.querySelector('style')
    expect(style).not.toBeNull()
    expect(style?.textContent).toContain('[data-chart=rev]')
    expect(style?.textContent).toContain('--color-desktop: #2563eb;')
    expect(style?.textContent).toContain('--color-mobile: #f97316;')
  })

  it('wraps the dark theme in a prefers-color-scheme media query', () => {
    const themed = {
      primary: { theme: { light: '#111111', dark: '#222222' } },
    } satisfies ChartConfig
    const { container } = render(<ChartStyle id="themed" config={themed} />)
    const style = container.querySelector('style')
    expect(style?.textContent).toContain('--color-primary: #111111;')
    expect(style?.textContent).toContain('@media (prefers-color-scheme: dark)')
    expect(style?.textContent).toContain('--color-primary: #222222;')
  })

  it('renders nothing when no config entry has a color or theme', () => {
    const { container } = render(<ChartStyle id="none" config={{ foo: { label: 'Foo' } }} />)
    expect(container.querySelector('style')).toBeNull()
  })
})

describe('ChartContainer', () => {
  it('renders a data-chart wrapper and injects ChartStyle when an id is set', () => {
    const { container } = render(
      <ChartContainer id="revenue" config={baseConfig}>
        <span>chart body</span>
      </ChartContainer>,
    )
    const chartEl = container.querySelector('[data-chart="chart-revenue"]')
    expect(chartEl).not.toBeNull()
    expect(chartEl).toHaveClass('flex')
    expect(chartEl).toHaveClass('aspect-video')
    expect(chartEl?.textContent).toContain('chart body')
    const style = container.querySelector('style')
    expect(style?.textContent).toContain('[data-chart=chart-revenue]')
    expect(style?.textContent).toContain('--color-desktop: #2563eb;')
    expect(screen.getByTestId('recharts-ResponsiveContainer')).toBeInTheDocument()
  })

  it('derives a chart id from useId when no id is provided', () => {
    const { container } = render(
      <ChartContainer config={baseConfig}>
        <span>chart body</span>
      </ChartContainer>,
    )
    const chartEl = container.querySelector('[data-chart]')
    expect(chartEl?.getAttribute('data-chart')).toMatch(/^chart-/)
    expect(container.querySelector('style')?.textContent).toContain('--color-desktop: #2563eb;')
  })

  it('forwards className and style onto the wrapper', () => {
    const { container } = render(
      <ChartContainer config={baseConfig} className="my-chart" style={{ width: '400px' }}>
        <span>chart body</span>
      </ChartContainer>,
    )
    const chartEl = container.querySelector('[data-chart]')
    expect(chartEl).toHaveClass('my-chart')
    expect((chartEl as HTMLElement).style.width).toBe('400px')
  })

  it('omits the style element when the config has no colors', () => {
    const { container } = render(
      <ChartContainer config={{}}>
        <span>chart body</span>
      </ChartContainer>,
    )
    expect(container.querySelector('[data-chart]')).not.toBeNull()
    expect(container.querySelector('style')).toBeNull()
  })
})

describe('ChartTooltip', () => {
  it('is wired to the mocked recharts Tooltip component', () => {
    renderInContainer(baseConfig, <ChartTooltip content={<ChartTooltipContent />} />)
    expect(screen.getByTestId('recharts-Tooltip')).toBeInTheDocument()
  })
})

describe('ChartTooltipContent', () => {
  it('renders the config label and a formatted value for an active single payload', () => {
    const payload = tooltipPayload(entry({ value: 1234 }))
    renderInContainer(baseConfig, <ChartTooltipContent active payload={payload} />)
    expect(screen.getAllByText('Desktop').length).toBe(2)
    expect(screen.getByText((1234).toLocaleString())).toBeInTheDocument()
    const swatch = document.querySelector('[style*="--color-bg"]')
    expect(swatch).not.toBeNull()
  })

  it('renders nothing when not active', () => {
    const payload = tooltipPayload(entry())
    renderInContainer(baseConfig, <ChartTooltipContent payload={payload} />)
    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
  })

  it('renders nothing when the payload is empty', () => {
    renderInContainer(baseConfig, <ChartTooltipContent active payload={tooltipPayload()} />)
    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
  })

  it('hides the label row when hideLabel is set', () => {
    const payload = tooltipPayload(entry())
    renderInContainer(baseConfig, <ChartTooltipContent active payload={payload} hideLabel />)
    expect(screen.getAllByText('Desktop').length).toBe(1)
  })

  it('applies the labelFormatter to the tooltip label', () => {
    const payload = tooltipPayload(entry())
    renderInContainer(
      baseConfig,
      <ChartTooltipContent
        active
        payload={payload}
        label="mobile"
        labelFormatter={(value: unknown) => `Formatted ${value}`}
      />,
    )
    expect(screen.getByText('Formatted Mobile')).toBeInTheDocument()
  })

  it('renders a custom formatter when the payload has a name and value', () => {
    const payload = tooltipPayload(entry({ value: 5 }))
    renderInContainer(
      baseConfig,
      <ChartTooltipContent
        active
        payload={payload}
        formatter={(value: unknown, name: unknown) => `Fmt ${name}:${value}`}
      />,
    )
    expect(screen.getByText('Fmt desktop:5')).toBeInTheDocument()
  })

  it('renders a line indicator and nests the label for a single payload', () => {
    const payload = tooltipPayload(entry())
    renderInContainer(baseConfig, <ChartTooltipContent active payload={payload} indicator="line" />)
    const swatch = document.querySelector('[style*="--color-bg"]')
    expect(swatch?.getAttribute('class')).toContain('w-1')
    const root = document.querySelector('[class*="min-w-[8rem]"]')
    // With a single non-dot payload the label is nested inside the row instead
    // of rendered as the first child of the tooltip container.
    expect(root?.firstElementChild?.classList.contains('font-medium')).toBe(false)
    expect(screen.getAllByText('Desktop').length).toBeGreaterThan(0)
  })

  it('renders a dashed indicator for a single payload', () => {
    const payload = tooltipPayload(entry())
    renderInContainer(
      baseConfig,
      <ChartTooltipContent active payload={payload} indicator="dashed" />,
    )
    const swatch = document.querySelector('[style*="--color-bg"]')
    expect(swatch?.getAttribute('class')).toContain('border-dashed')
  })

  it('omits the indicator swatch when hideIndicator is set', () => {
    const payload = tooltipPayload(entry())
    renderInContainer(baseConfig, <ChartTooltipContent active payload={payload} hideIndicator />)
    expect(document.querySelector('[style*="--color-bg"]')).toBeNull()
    expect(screen.getAllByText('Desktop').length).toBe(2)
  })

  it('renders the config icon instead of the indicator when one is configured', () => {
    const iconConfig = {
      desktop: { label: 'Desktop', icon: () => <span data-testid="tooltip-icon" /> },
    } satisfies ChartConfig
    const payload = tooltipPayload(entry())
    renderInContainer(iconConfig, <ChartTooltipContent active payload={payload} />)
    expect(screen.getByTestId('tooltip-icon')).toBeInTheDocument()
  })

  it('resolves the config entry through nameKey', () => {
    const payload = tooltipPayload(entry())
    const config = { other: { label: 'Other', color: '#000000' } } satisfies ChartConfig
    renderInContainer(config, <ChartTooltipContent active payload={payload} nameKey="other" />)
    expect(screen.getByText('Other')).toBeInTheDocument()
  })
})

describe('ChartLegend', () => {
  it('is wired to the mocked recharts Legend component', () => {
    renderInContainer(baseConfig, <ChartLegend content={<ChartLegendContent />} />)
    expect(screen.getByTestId('recharts-Legend')).toBeInTheDocument()
  })
})

describe('ChartLegendContent', () => {
  it('renders a color swatch and label per payload entry', () => {
    const payload = legendPayload(
      { dataKey: 'desktop', value: 'desktop', color: '#2563eb' },
      { dataKey: 'mobile', value: 'mobile', color: '#f97316' },
    )
    renderInContainer(baseConfig, <ChartLegendContent payload={payload} />)
    expect(screen.getByText('Desktop')).toBeInTheDocument()
    expect(screen.getByText('Mobile')).toBeInTheDocument()
    const swatches = document.querySelectorAll('[class*="shrink-0 rounded-[2px]"]')
    expect(swatches.length).toBe(2)
  })

  it('applies the top alignment class for verticalAlign top', () => {
    const payload = legendPayload({ dataKey: 'desktop', value: 'desktop', color: '#2563eb' })
    const { container } = renderInContainer(
      baseConfig,
      <ChartLegendContent payload={payload} verticalAlign="top" />,
    )
    expect(container.querySelector('[class*="pb-3"]')).not.toBeNull()
    expect(container.querySelector('[class*="pt-3"]')).toBeNull()
  })

  it('renders nothing when the payload is empty', () => {
    renderInContainer(baseConfig, <ChartLegendContent payload={legendPayload()} />)
    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
  })

  it('resolves the config entry through nameKey', () => {
    const payload = legendPayload({ dataKey: 'desktop', value: 'desktop', color: '#000000' })
    const config = { other: { label: 'Other', color: '#000000' } } satisfies ChartConfig
    renderInContainer(config, <ChartLegendContent payload={payload} nameKey="other" />)
    expect(screen.getByText('Other')).toBeInTheDocument()
  })

  it('renders the config icon unless hideIcon is set', () => {
    const iconConfig = {
      desktop: { label: 'Desktop', icon: () => <span data-testid="legend-icon" /> },
    } satisfies ChartConfig
    const payload = legendPayload({ dataKey: 'desktop', value: 'desktop', color: '#000000' })

    const { rerender } = renderInContainer(iconConfig, <ChartLegendContent payload={payload} />)
    expect(screen.getByTestId('legend-icon')).toBeInTheDocument()

    rerender(
      <ChartContainer config={iconConfig}>
        <ChartLegendContent payload={payload} hideIcon />
      </ChartContainer>,
    )
    expect(screen.queryByTestId('legend-icon')).not.toBeInTheDocument()
    expect(screen.getByText('Desktop')).toBeInTheDocument()
  })
})
