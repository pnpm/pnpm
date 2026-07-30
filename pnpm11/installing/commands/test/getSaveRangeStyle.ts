import { expect, test } from '@jest/globals'

import { getSaveRangeStyle } from '../lib/getSaveRangeStyle.js'

test('getSaveRangeStyle()', () => {
  expect(getSaveRangeStyle({ saveExact: true })).toBe('patch')
  expect(getSaveRangeStyle({ savePrefix: '' })).toBe('patch')
  expect(getSaveRangeStyle({ savePrefix: '~' })).toBe('minor')
  expect(getSaveRangeStyle({ savePrefix: '^' })).toBe('major')
})
