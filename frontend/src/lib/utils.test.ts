import { describe, expect, it } from 'vitest'
import { cn } from './utils'

describe('cn', () => {
  it('merges conflicting tailwind classes (last one wins)', () => {
    expect(cn('px-2', 'px-4')).toBe('px-4')
  })

  it('merges classes from different groups', () => {
    expect(cn('text-red-500', { 'bg-blue-500': true })).toBe('text-red-500 bg-blue-500')
  })

  it('ignores falsy class values', () => {
    expect(cn(false, null, undefined, 'block')).toBe('block')
  })
})
