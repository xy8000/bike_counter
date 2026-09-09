import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TotalBikesCard } from './TotalBikesCard'

describe('TotalBikesCard', () => {
  it('renders the all-time total with the bikes unit', () => {
    render(<TotalBikesCard total={1234} />)

    expect(screen.getByText('Total bikes (all time)')).toBeInTheDocument()
    expect(screen.getByText('1.234')).toBeInTheDocument()
    expect(screen.getByText('bikes')).toBeInTheDocument()
  })
})
