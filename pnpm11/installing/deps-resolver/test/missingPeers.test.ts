import { expect, test } from '@jest/globals'

import { hoistPeers } from '../lib/hoistPeers.js'
import { mergePkgsDeps } from '../lib/missingPeers.js'

test.each([
  'file:../b',
  'link:../b',
  '../b',
  './b',
  '/packages/b',
  '~/b',
  'npm:other@1.0.0',
  'workspace:*',
  'workspace:other@^1',
  'http://example.test/b.tgz',
  'https://example.test/b.tgz',
  'git+https://example.test/b.git',
  'git+ssh://git@example.test/b.git',
  'github:example/b',
  'gitlab:example/b',
  'bitbucket:example/b',
  'example/b',
  'catalog:default',
  'work:^1.0.0',
  'work:other@1.0.0',
])(
  'preserves merged peer locator %s when registry versions are preferred',
  (range) => {
    for (const autoInstallPeersFromHighestMatch of [false, true]) {
      const merged = mergePkgsDeps([
        { b: { range, optional: false } },
        { b: { range, optional: false } },
      ], { autoInstallPeersFromHighestMatch })
      for (const autoInstallPeers of [false, true]) {
        expect(hoistPeers({
          autoInstallPeers,
          allPreferredVersions: {
            b: {
              '1.0.0': 'version',
              '2.0.0': { selectorType: 'version', weight: 1000 },
            },
          },
          workspaceRootDeps: [],
        }, Object.entries(merged))).toStrictEqual(autoInstallPeers ? { b: range } : {})
      }
    }
  }
)

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

test.each(['latest', 'next'])('still deduplicates registry tag %s onto a preferred version', (range) => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: { b: { '1.0.0': 'version' } },
    workspaceRootDeps: [],
  }, [['b', { range }]])).toStrictEqual({ b: '1.0.0' })
})
