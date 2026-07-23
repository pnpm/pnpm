import { expect, test } from '@jest/globals'

import { getHoistableOptionalPeers, hoistPeers } from '../lib/hoistPeers.js'

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

test('hoistPeers picks the highest preferred version that satisfies a range for deduplication', () => {
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
    foo: '2.1.0',
  })
})

test('hoistPeers does not reuse a preferred version that the peer range rejects', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '1' }]])).toStrictEqual({
    foo: '1',
  })
})

test('hoistPeers prefers the preferred version that satisfies a non-exact range', () => {
  // In a multi-importer workspace, allPreferredVersions aggregates versions
  // from every importer. A peer declared as ^1.0.0 must not be handed a
  // foreign 2.x contributed by another importer when a satisfying 1.x exists.
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '1.0.0': 'version',
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '^1.0.0' }]])).toStrictEqual({
    foo: '1.0.0',
  })
})

test('hoistPeers does not treat a prerelease of the next major as satisfying a caret range', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '1.0.0': 'version',
        '2.0.0-beta.1': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '^1.0.0' }]])).toStrictEqual({
    foo: '1.0.0',
  })
})

test('hoistPeers falls back to the range when no preferred version satisfies a non-exact range', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '^1.0.0' }]])).toStrictEqual({
    foo: '^1.0.0',
  })
})

test('hoistPeers hoists nothing when no preferred version satisfies the range and peers are not auto-installed', () => {
  expect(hoistPeers({
    autoInstallPeers: false,
    allPreferredVersions: {
      foo: {
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '^1.0.0' }]])).toStrictEqual({})
})

// Regression test for https://github.com/pnpm/pnpm/pull/11049
test('hoistPeers returns valid specifier when given only range preferred version selectors', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '^2.0.0': 'range',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '2' }]])).toStrictEqual({
    foo: '^2.0.0',
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

test('hoistPeers dedupes a named-registry peer onto a preferred version that satisfies its extracted range', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '1.0.0': 'version',
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: 'work:^1.0.0' }]])).toStrictEqual({
    foo: '1.0.0',
  })
})

test('hoistPeers falls back to the raw scheme specifier when no preferred version satisfies its extracted range', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '2.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: 'work:^1.0.0' }]])).toStrictEqual({
    foo: 'work:^1.0.0',
  })
})

test('hoistPeers respects a merged || union of scheme specifiers instead of picking the highest version', () => {
  // `4.0.0` is the highest but satisfies neither `^2.0.0` nor `^3.0.0`, so a
  // blind highest-version pick would be wrong; `3.0.0` is the highest match.
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '2.1.0': 'version',
        '3.0.0': 'version',
        '4.0.0': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: 'work:^2.0.0 || work:^3.0.0' }]])).toStrictEqual({
    foo: '3.0.0',
  })
})

// Regression test for https://github.com/pnpm/pnpm/pull/11048
test('hoistPeers handles version selector with weight', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      foo: {
        '1.0.0': { selectorType: 'version', weight: 1 },
      },
    },
    workspaceRootDeps: [],
  }, [['foo', { range: '1' }]])).toStrictEqual({
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

test('getHoistableOptionalPeers handles version selector with weight', () => {
  expect(getHoistableOptionalPeers({
    jsdom: ['*'],
  }, {
    jsdom: {
      '26.1.0': 'version',
      '27.4.0': { selectorType: 'version', weight: 1 },
    },
  })).toStrictEqual({
    jsdom: '27.4.0',
  })
})
