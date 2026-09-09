import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { MAX_DATA_STREAMS } from './chartUtils'
import { ChartLimitNotice } from './ChartLimitNotice'

describe('ChartLimitNotice', () => {
  it('renders the default too-many-data-streams message', () => {
    render(<ChartLimitNotice />)
    expect(
      screen.getByText(
        `This chart cannot be loaded — too many data-streams to render (max ${MAX_DATA_STREAMS}).`,
      ),
    ).toBeInTheDocument()
  })

  it('renders a custom message and forwards the className', () => {
    const { container } = render(<ChartLimitNotice message="Too many!" className="py-16" />)
    expect(screen.getByText('Too many!')).toBeInTheDocument()
    expect(container.firstChild).toHaveClass('flex', 'items-center', 'justify-center', 'py-16')
  })
})
