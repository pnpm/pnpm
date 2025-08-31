import { getPinnedVersion } from '../lib/getPinnedVersion.js'

test('getPinnedVersion()', () => {
  expect(getPinnedVersion({ saveExact: true })).toBe('patch')
  expect(getPinnedVersion({ savePrefix: '' })).toBe('patch')
  expect(getPinnedVersion({ savePrefix: '~' })).toBe('minor')
  expect(getPinnedVersion({ savePrefix: '^' })).toBe('major')
})
