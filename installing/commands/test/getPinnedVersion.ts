import { getPinnedVersion } from '../lib/getPinnedVersion.js'
import { expect, test } from '@jest/globals'

test('getPinnedVersion()', () => {
  expect(getPinnedVersion({ saveExact: true })).toBe('patch')
  expect(getPinnedVersion({ savePrefix: '' })).toBe('patch')
  expect(getPinnedVersion({ savePrefix: '~' })).toBe('minor')
  expect(getPinnedVersion({ savePrefix: '^' })).toBe('major')
})
