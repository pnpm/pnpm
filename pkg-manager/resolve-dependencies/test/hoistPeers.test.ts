import { hoistPeers, getHoistableOptionalPeers } from '../lib/hoistPeers'

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

test('getHoistableOptionalPeers only picks a version that satisfies all optional ranges', () => {
  expect(getHoistableOptionalPeers({
    foo: ['2', '2.1'],
  }, {
    foo: {
      '1.0.0': 'version',
      '2.0.0': 'version',
      '2.1.0': 'version',
      '3.0.0': 'version',
    },
  })).toStrictEqual({
    foo: '2.1.0',
  })
})

test('getHoistableOptionalPeers picks the highest version that satisfies all the optional ranges', () => {
  expect(getHoistableOptionalPeers({
    foo: ['2', '2.1'],
  }, {
    foo: {
      '2.1.0': 'version',
      '2.1.1': 'version',
    },
  })).toStrictEqual({
    foo: '2.1.1',
  })
})
