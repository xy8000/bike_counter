import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { formatFullDate, formatNumber } from '../../lib/format'
import { computeKeyFacts, KeyFacts, type KeyFactsInput } from './KeyFacts'
import type { HourTotal, TimeBucket, WeekdayTotal } from './types'

const bucket = (start: string, total: number): TimeBucket => ({ start, total })
const hour = (hour: number, total: number): HourTotal => ({ hour, total })
const weekday = (weekday: number, total: number): WeekdayTotal => ({ weekday, total })

describe('computeKeyFacts', () => {
  it('returns no facts when there is no traffic at all', () => {
    const facts = computeKeyFacts({ current: [], hourly: [], weekday_radar: [] })
    expect(facts).toEqual([])
  })

  it('computes the total bikes and the busiest local day', () => {
    // Noon UTC keeps every bucket on the same browser-local calendar day.
    const current = [
      bucket('2024-01-01T12:00:00.000Z', 5),
      bucket('2024-01-02T12:00:00.000Z', 9),
      bucket('2024-01-02T15:00:00.000Z', 4),
      bucket('2024-01-03T12:00:00.000Z', 2),
    ]
    const facts = computeKeyFacts({ current, hourly: [], weekday_radar: [] })

    const total = facts.find((fact) => fact.key === 'total_bikes')
    expect(total?.label).toBe('Total bikes in selection')
    expect(total?.value).toBe(formatNumber(20))
    expect(total?.unit).toBe('bikes')

    const day = facts.find((fact) => fact.key === 'busiest_day')
    expect(day?.label).toBe('Busiest day in range')
    expect(day?.value).toBe(formatFullDate(new Date(2024, 0, 2).getTime()))
    expect(day?.detail).toBe(`${formatNumber(13)} bikes`)
  })

  it('picks the busiest hour and the busiest weekday', () => {
    const facts = computeKeyFacts({
      current: [bucket('2024-01-01T12:00:00.000Z', 10)],
      hourly: [hour(8, 3), hour(17, 12), hour(20, 8)],
      weekday_radar: [weekday(1, 2), weekday(3, 9), weekday(6, 30)],
    })

    const peakHour = facts.find((fact) => fact.key === 'busiest_hour')
    expect(peakHour?.label).toBe('Busiest hour')
    expect(peakHour?.detail).toBe(`${formatNumber(12)} bikes`)
    expect(peakHour?.value.length).toBeGreaterThan(0)

    const peakWeekday = facts.find((fact) => fact.key === 'busiest_weekday')
    expect(peakWeekday?.label).toBe('Busiest weekday')
    expect(peakWeekday?.detail).toBe(`${formatNumber(30)} bikes`)
    expect(peakWeekday?.value.length).toBeGreaterThan(0)
  })

  it('keeps the first entry when values tie', () => {
    const facts = computeKeyFacts({
      current: [bucket('2024-01-01T12:00:00.000Z', 10)],
      hourly: [hour(6, 5), hour(9, 5), hour(12, 5)],
      weekday_radar: [weekday(2, 7), weekday(4, 7)],
    })

    const peakHour = facts.find((fact) => fact.key === 'busiest_hour')
    // 06:00 is the first of the tied hours and wins.
    expect(peakHour?.detail).toBe(`${formatNumber(5)} bikes`)
    expect(peakHour?.value).toContain('6')

    const peakWeekday = facts.find((fact) => fact.key === 'busiest_weekday')
    expect(peakWeekday?.detail).toBe(`${formatNumber(7)} bikes`)
  })

  it('skips the hour/weekday facts when those inputs are empty or all-zero', () => {
    const allZero: KeyFactsInput = {
      current: [bucket('2024-01-01T12:00:00.000Z', 10)],
      hourly: [hour(7, 0), hour(8, 0)],
      weekday_radar: [weekday(1, 0)],
    }
    const facts = computeKeyFacts(allZero)

    expect(facts.some((fact) => fact.key === 'total_bikes')).toBe(true)
    expect(facts.some((fact) => fact.key === 'busiest_day')).toBe(true)
    expect(facts.some((fact) => fact.key === 'busiest_hour')).toBe(false)
    expect(facts.some((fact) => fact.key === 'busiest_weekday')).toBe(false)
  })
})

describe('KeyFacts', () => {
  it('renders nothing when there are no facts', () => {
    const { container } = render(<KeyFacts facts={[]} />)
    expect(container.firstChild).toBeNull()
  })

  it('renders each fact box with label, value, unit and detail', () => {
    render(
      <KeyFacts
        facts={[
          { key: 'total_bikes', label: 'Total bikes', value: '1.234', unit: 'bikes' },
          { key: 'busiest_day', label: 'Busiest day', value: '02.01.2024', detail: '13 bikes' },
        ]}
      />,
    )

    expect(screen.getByText('Total bikes')).toBeInTheDocument()
    expect(screen.getByText('1.234')).toBeInTheDocument()
    expect(screen.getByText('bikes')).toBeInTheDocument()
    expect(screen.getByText('Busiest day')).toBeInTheDocument()
    expect(screen.getByText('02.01.2024')).toBeInTheDocument()
    expect(screen.getByText('13 bikes')).toBeInTheDocument()
  })

  it('renders a fact without a unit or detail', () => {
    render(<KeyFacts facts={[{ key: 'busiest_day', label: 'Busiest day', value: '02.01.2024' }]} />)
    expect(screen.getByText('Busiest day')).toBeInTheDocument()
    expect(screen.getByText('02.01.2024')).toBeInTheDocument()
  })
})
