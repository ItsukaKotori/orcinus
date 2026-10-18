import { expect, it } from 'vitest'
import { formatRelativeDate, shortHash } from './format'

it('formatRelativeDate 按阈值输出相对时间', () => {
  const now = Date.UTC(2026, 9, 18, 12)
  expect(formatRelativeDate(now - 30_000, now)).toBe('just now')
  expect(formatRelativeDate(now - 5 * 60_000, now)).toBe('5m ago')
  expect(formatRelativeDate(now - 3 * 3_600_000, now)).toBe('3h ago')
  expect(formatRelativeDate(now - 2 * 86_400_000, now)).toBe('2d ago')
  expect(formatRelativeDate(undefined, now)).toBe('')
})
it('shortHash 取前 7 位', () => expect(shortHash('abcdef0123456789')).toBe('abcdef0'))
