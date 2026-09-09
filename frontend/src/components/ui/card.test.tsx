import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from './card'

describe('Card', () => {
  it('renders a container div with data-slot="card"', () => {
    render(<Card>Body</Card>)
    expect(screen.getByText('Body')).toHaveAttribute('data-slot', 'card')
  })

  it('merges an extra className onto the card', () => {
    render(<Card className="my-card">x</Card>)
    expect(screen.getByText('x')).toHaveClass('my-card')
  })

  it('renders header/title/description/content/footer sections with children', () => {
    render(
      <Card>
        <CardHeader>
          <CardTitle>My Title</CardTitle>
          <CardDescription>A description</CardDescription>
        </CardHeader>
        <CardContent>Content</CardContent>
        <CardFooter>
          <button type="button">Save</button>
        </CardFooter>
      </Card>,
    )

    expect(screen.getByText('My Title')).toHaveAttribute('data-slot', 'card-title')
    expect(screen.getByText('A description')).toHaveAttribute('data-slot', 'card-description')
    expect(screen.getByText('Content')).toHaveAttribute('data-slot', 'card-content')

    const title = screen.getByText('My Title')
    expect(title.closest('[data-slot="card-header"]')).not.toBeNull()
    expect(title.closest('[data-slot="card"]')).not.toBeNull()

    const footer = screen.getByRole('button', { name: 'Save' })
    expect(footer.closest('[data-slot="card-footer"]')).not.toBeNull()
  })

  it('renders CardAction inside a card header', () => {
    render(
      <Card>
        <CardHeader>
          <CardTitle>t</CardTitle>
          <CardAction>action</CardAction>
        </CardHeader>
      </Card>,
    )
    const action = screen.getByText('action')
    expect(action).toHaveAttribute('data-slot', 'card-action')
    expect(action.closest('[data-slot="card-header"]')).not.toBeNull()
  })
})
