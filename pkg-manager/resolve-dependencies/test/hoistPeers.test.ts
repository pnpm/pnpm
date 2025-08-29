import { hoistPeers, getHoistableOptionalPeers } from '../lib/hoistPeers.js'

test('hoistPeers picks an already available prerelease version', () => {
  expect(hoistPeers({
    autoInstallPeers: false,
    allPreferredVersions: {
      foo: {
        '1.0.0-beta.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '*' }]])).toStrictEqual({
    foo: '1.0.0-beta.0',
  })
})

test('hoistPeers uses workspace root dependency when not overridden', () => {
  expect(hoistPeers({
    autoInstallPeers: false,
    allPreferredVersions: {},
    workspaceRootDeps: [{
      alias: 'foo',
      version: '1.0.0',
      pkgId: 'foo@1.0.0' as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      depIsLinked: false,
      isNew: false,
      isLinkedDependency: undefined,
      nodeId: 'foo@1.0.0' as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      installable: true,
      pkg: { name: 'foo', version: '1.0.0' },
      updated: false,
      rootDir: '/test',
      missingPeers: {},
      optional: false,
    }],
  }, [['foo', { range: '*' }]])).toStrictEqual({
    foo: '1.0.0',
  })
})

test('hoistPeers skips workspace root dependency when overridden', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {},
    workspaceRootDeps: [{
      alias: 'foo',
      version: '1.0.0',
      pkgId: 'bar@1.0.0' as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      depIsLinked: false,
      isNew: false,
      isLinkedDependency: undefined,
      nodeId: 'bar@1.0.0' as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      installable: true,
      pkg: { name: 'bar', version: '1.0.0' },
      updated: false,
      rootDir: '/test',
      missingPeers: {},
      optional: false,
    }],
  }, [['foo', { range: '^1.0.0' }]])).toStrictEqual({
    foo: '^1.0.0', // Falls back to the peer range
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
