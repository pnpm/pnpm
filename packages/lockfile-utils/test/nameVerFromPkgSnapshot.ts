import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'

test('nameVerFromPkgSnapshot()', () => {
  expect(nameVerFromPkgSnapshot('/some-weird-path', {
    name: 'foo',
    version: '1.0.0',

    resolution: {
      integrity: 'AAA',
    },
  })).toEqual({
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(nameVerFromPkgSnapshot('/foo/1.0.0', {
    resolution: {
      integrity: 'AAA',
    },
  })).toEqual({
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(nameVerFromPkgSnapshot('/foo/1.0.0_bar@2.0.0', {
    resolution: {
      integrity: 'AAA',
    },
  })).toEqual({
    name: 'foo',
    peersSuffix: 'bar@2.0.0',
    version: '1.0.0',
  })
})
