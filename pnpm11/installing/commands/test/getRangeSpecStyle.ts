import { expect, test } from '@jest/globals'

import { getRangeSpecStyle } from '../lib/getRangeSpecStyle.js'

test('getRangeSpecStyle()', () => {
  expect(getRangeSpecStyle({ saveExact: true })).toBe('patch')
  expect(getRangeSpecStyle({ savePrefix: '' })).toBe('patch')
  expect(getRangeSpecStyle({ savePrefix: '~' })).toBe('minor')
  expect(getRangeSpecStyle({ savePrefix: '^' })).toBe('major')
})
