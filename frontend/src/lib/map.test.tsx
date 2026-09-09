import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { activeFlagUrl, inactiveFlagUrl, selectedFlagUrl, stationMarkerImage } from './map'

function flagFor(state: 'active' | 'selected' | 'inactive'): string {
  return stationMarkerImage('Station', { state }).props.src as string
}

describe('exported flag URLs', () => {
  it('exposes non-empty asset URLs for all three flag variants', () => {
    expect(activeFlagUrl).toEqual(expect.any(String))
    expect(selectedFlagUrl).toEqual(expect.any(String))
    expect(inactiveFlagUrl).toEqual(expect.any(String))
    expect(activeFlagUrl.length).toBeGreaterThan(0)
    expect(selectedFlagUrl.length).toBeGreaterThan(0)
    expect(inactiveFlagUrl.length).toBeGreaterThan(0)
  })
})

describe('stationMarkerImage', () => {
  it('builds an active marker image by default with name in alt and title', () => {
    const { rerender } = render(<div>{stationMarkerImage('Alpha')}</div>)
    const img = screen.getByRole('img', { name: 'Alpha' })

    expect(img).toHaveAttribute('src', activeFlagUrl)
    expect(img).toHaveAttribute('title', 'Alpha')
    expect(img).toHaveClass('station-marker')
    expect(img).not.toHaveClass('station-marker--selected')
    expect(img).not.toHaveClass('station-marker--inactive')

    rerender(<div>{stationMarkerImage('Beta')}</div>)
    expect(screen.getByRole('img', { name: 'Beta' })).toHaveAttribute('title', 'Beta')
  })

  it('selects the flag URL by state', () => {
    expect(flagFor('active')).toBe(activeFlagUrl)
    expect(flagFor('selected')).toBe(selectedFlagUrl)
    expect(flagFor('inactive')).toBe(inactiveFlagUrl)
  })

  it('adds the selected variant class when state is selected', () => {
    render(<div>{stationMarkerImage('Alpha', { state: 'selected' })}</div>)
    const img = screen.getByRole('img', { name: 'Alpha' })

    expect(img).toHaveClass('station-marker', 'station-marker--selected')
    expect(img).not.toHaveClass('station-marker--inactive')
  })

  it('adds the inactive variant class when state is inactive', () => {
    render(<div>{stationMarkerImage('Alpha', { state: 'inactive' })}</div>)
    const img = screen.getByRole('img', { name: 'Alpha' })

    expect(img).toHaveClass('station-marker', 'station-marker--inactive')
    expect(img).not.toHaveClass('station-marker--selected')
  })

  it('adds the disabled and large modifier classes', () => {
    render(<div>{stationMarkerImage('Alpha', { disabled: true, large: true })}</div>)
    const img = screen.getByRole('img', { name: 'Alpha' })

    expect(img).toHaveClass('station-marker', 'station-marker--disabled', 'station-marker--large')
    expect(img).not.toHaveClass('station-marker--selected')
    expect(img).not.toHaveClass('station-marker--inactive')
  })

  it('marks the image draggable false so marker dragging never starts', () => {
    render(<div>{stationMarkerImage('Alpha')}</div>)
    expect(screen.getByRole('img', { name: 'Alpha' })).toHaveAttribute('draggable', 'false')
  })
})
