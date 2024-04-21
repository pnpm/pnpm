import { hoistPeers } from '../lib/hoistPeers'

test('hoistPeers picks an already available prerelease version', () => {
  expect(hoistPeers([['foo', { range: '*' }]], {
    autoInstallPeers: false,
    allPreferredVersions: {
      foo: {
        '1.0.0-beta.0': 'version',
      },
    },
  })).toStrictEqual({
    foo: '1.0.0-beta.0',
  })
})
