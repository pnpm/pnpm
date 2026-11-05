import { expect, test } from '@jest/globals'

import { mergePkgsDeps } from '../lib/missingPeers.js'

test.each([
  ['file:../packages/b', 'file:../libs/b'],
  ['link:../b', 'link:../c'],
  ['../b', '../c'],
  ['file:../b', '^1.0.0'],
  ['^1.0.0', 'file:../b'],
  ['npm:b@^1', 'npm:b@^2'],
  ['latest', 'next'],
])('does not join conflicting non-semver peer ranges %s and %s', (first, second) => {
  for (const autoInstallPeersFromHighestMatch of [false, true]) {
    expect(mergePkgsDeps([
      { b: { range: first, optional: false } },
      { b: { range: second, optional: false } },
    ], { autoInstallPeersFromHighestMatch })).toStrictEqual({})
  }
})

test.each(['file:../b', 'link:../b', '../b', 'npm:b@^1', 'latest'])(
  'preserves repeated non-semver peer range %s',
  (range) => {
    for (const autoInstallPeersFromHighestMatch of [false, true]) {
      expect(mergePkgsDeps([
        { b: { range, optional: true } },
        { b: { range, optional: false } },
      ], { autoInstallPeersFromHighestMatch })).toStrictEqual({
        b: { range, optional: false },
      })
    }
  }
)

test('joins conflicting semver peer ranges only with highest-match enabled', () => {
  const peers = [
    { b: { range: '^1.0.0', optional: false } },
    { b: { range: '^2.0.0', optional: false } },
  ]
  expect(mergePkgsDeps(peers, { autoInstallPeersFromHighestMatch: false })).toStrictEqual({})
  expect(mergePkgsDeps(peers, { autoInstallPeersFromHighestMatch: true })).toStrictEqual({
    b: { range: '^1.0.0 || ^2.0.0', optional: false },
  })
})
