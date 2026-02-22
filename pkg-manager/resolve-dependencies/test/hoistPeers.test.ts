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

test('hoistPeers respects peer dep range when preferred versions exist', () => {
  // When an override narrows a peer dep range (e.g. chai: "4.3.0"),
  // we should not pick a preferred version that doesn't satisfy it.
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      chai: {
        '5.2.1': 'version',
        '4.3.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['chai', { range: '4.3.0' }]])).toStrictEqual({
    chai: '4.3.0',
  })
})

test('hoistPeers falls back to range when no preferred version satisfies it', () => {
  // When no preferred version satisfies the overridden range,
  // fall back to the range itself so pnpm resolves from the registry.
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      chai: {
        '5.2.1': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['chai', { range: '4.3.0' }]])).toStrictEqual({
    chai: '4.3.0',
  })
})

test('hoistPeers picks highest preferred version for deduplication when range is not exact', () => {
  // For non-exact ranges (like ^2.0.0), hoistPeers picks the highest preferred
  // version overall (for deduplication), not just the highest satisfying the range.
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '2.0.0': 'version',
        '2.1.0': 'version',
        '3.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '^2.0.0' }]])).toStrictEqual({
    foo: '3.0.0',
  })
})

test('hoistPeers reuses higher preferred version when range is not exact', () => {
  // When the peer dep range is a semver range (not an exact version),
  // prefer reusing a higher existing version for deduplication even if
  // it doesn't satisfy the range.
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '1' }]])).toStrictEqual({
    foo: '2.0.0',
  })
})

test('hoistPeers handles workspace: protocol range without throwing', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '1.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: 'workspace:*' }]])).toStrictEqual({
    foo: '1.0.0',
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
