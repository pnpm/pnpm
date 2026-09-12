import { expect, test } from '@jest/globals'
import { lexCompare } from '@pnpm/text.ordinal-comparator'

test('lexCompare', () => {
  expect(lexCompare('a', 'b')).toBe(-1)
  expect(lexCompare('a', 'a')).toBe(0)
  expect(lexCompare('b', 'a')).toBe(1)
})

test('lexCompare sorts uppercase letters before lowercase ones', () => {
  expect(['b', 'A', 'a', 'B'].sort(lexCompare)).toStrictEqual(['A', 'B', 'a', 'b'])
})
