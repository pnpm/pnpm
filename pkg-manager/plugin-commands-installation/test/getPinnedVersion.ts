import { getPinnedVersion } from '../lib/getPinnedVersion'

test('getPinnedVersion()', () => {
  expect(getPinnedVersion({ saveExact: true })).toEqual('patch')
  expect(getPinnedVersion({ savePrefix: '' })).toEqual('patch')
  expect(getPinnedVersion({ savePrefix: '~' })).toEqual('minor')
  expect(getPinnedVersion({ savePrefix: '^' })).toEqual('major')
})
