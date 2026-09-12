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

// `includePrerelease` keeps a prerelease eligible for the comparators it
// falls between; it does not lower a bound the range spells out. `^18.0.0`
// therefore still starts at 18.0.0, and hoisting the range lets a stable
// 18.x be resolved from the registry.
test('hoistPeers rejects a prerelease below a spelled-out lower bound', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      react: {
        '18.0.0-rc.1': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['react', { range: '^18.0.0' }]])).toStrictEqual({
    react: '^18.0.0',
  })
})

test('hoistPeers accepts a prerelease below a lower bound synthesized from an omitted component', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      react: {
        '18.0.0-rc.1': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['react', { range: '^18.x' }]])).toStrictEqual({
    react: '18.0.0-rc.1',
  })
})

test('hoistPeers accepts a prerelease inside the span of a union', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      react: {
        '19.3.0-canary-28cd4bb0-20260723': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['react', { range: '^18.x || ^19.x' }]])).toStrictEqual({
    react: '19.3.0-canary-28cd4bb0-20260723',
  })
})

test('hoistPeers rejects a prerelease at the lower bound of a union', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      'jest-util': {
        '30.0.0-alpha.6': 'version',
      },
    },
    workspaceRootDeps: [],
  }, [['jest-util', { range: '^29.0.0 || ^30.0.0' }]])).toStrictEqual({
    'jest-util': '^29.0.0 || ^30.0.0',
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

test('hoistPeers installs an auto-installed peer at the overridden specifier', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      react: {
        '18.3.1': 'version',
      },
    },
    workspaceRootDeps: [{ alias: 'react', pkgName: 'react', normalizedBareSpecifier: '18.3.1' }],
    overrideBareSpecifier: (name) => name === 'react' ? 'npm:react@19.2.0' : undefined,
  }, [['react', { range: '^16.5.1 || ^17.0.0 || ^18.0.0' }]])).toStrictEqual({
    react: 'npm:react@19.2.0',
  })
})

test('hoistPeers does not let an override install a peer that nothing provides when peers are not auto-installed', () => {
  expect(hoistPeers({
    autoInstallPeers: false,
    allPreferredVersions: {},
    workspaceRootDeps: [],
    overrideBareSpecifier: () => 'npm:react@19.2.0',
  }, [['react', { range: '^18.0.0' }]])).toStrictEqual({})
})

test('hoistPeers leaves a deduplicating hoist to the graph when peers are not auto-installed', () => {
  expect(hoistPeers({
    autoInstallPeers: false,
    allPreferredVersions: {
      react: {
        '18.3.1': 'version',
      },
    },
    workspaceRootDeps: [],
    overrideBareSpecifier: () => 'npm:react@19.2.0',
  }, [['react', { range: '^18.0.0' }]])).toStrictEqual({
    react: '18.3.1',
  })
})

test('hoistPeers redirects the workspace root\'s hoist through an override when peers are not auto-installed', () => {
  expect(hoistPeers({
    autoInstallPeers: false,
    allPreferredVersions: {},
    workspaceRootDeps: [{ alias: 'react', pkgName: 'react', normalizedBareSpecifier: '18.3.1' }],
    overrideBareSpecifier: () => 'npm:react@19.2.0',
  }, [['react', { range: '^18.0.0' }]])).toStrictEqual({
    react: 'npm:react@19.2.0',
  })
})

test('hoistPeers leaves a peer removed by an override uninstalled', () => {
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {
      react: {
        '18.3.1': 'version',
      },
    },
    workspaceRootDeps: [],
    overrideBareSpecifier: () => '-',
  }, [['react', { range: '^18.0.0' }]])).toStrictEqual({})
})

test('getHoistableOptionalPeers stays within the workspace root\'s range', () => {
  const allMissingOptionalPeers = { postcss: ['*'] }
  const allPreferredVersions = {
    postcss: {
      '8.5.10': 'version' as const,
      '8.5.22': 'version' as const,
    },
  }
  expect(getHoistableOptionalPeers(allMissingOptionalPeers, allPreferredVersions, [
    { alias: 'postcss', pkgName: 'postcss', normalizedBareSpecifier: '8.5.10' },
  ])).toStrictEqual({
    postcss: '8.5.10',
  })
  expect(getHoistableOptionalPeers(allMissingOptionalPeers, allPreferredVersions)).toStrictEqual({
    postcss: '8.5.22',
  })
})

test('getHoistableOptionalPeers ignores a workspace root specifier that a wanted range rejects', () => {
  expect(getHoistableOptionalPeers({ 'date-fns': ['^4.0.0'] }, {
    'date-fns': {
      '2.30.0': 'version',
      '4.4.0': 'version',
    },
  }, [
    { alias: 'date-fns-v2', pkgName: 'date-fns', normalizedBareSpecifier: 'npm:date-fns@2.30.0' },
  ])).toStrictEqual({
    'date-fns': '4.4.0',
  })
})

test('getHoistableOptionalPeers ignores a workspace root specifier that only one wanted range accepts', () => {
  expect(getHoistableOptionalPeers({ foo: ['>=1.0.0 <3.0.0', '>=2.0.0 <4.0.0'] }, {
    foo: {
      '2.0.0': 'version',
    },
  }, [
    { alias: 'foo', pkgName: 'foo', normalizedBareSpecifier: '1.0.0' },
  ])).toStrictEqual({
    foo: '2.0.0',
  })
})

test('hoistPeers skips a workspace root dependency that has no specifier in favor of one that has', () => {
  const workspaceRootDeps = [
    { alias: 'postcss', pkgName: 'postcss' },
    { alias: 'zz-postcss', pkgName: 'postcss', normalizedBareSpecifier: '8.5.10' },
  ]
  expect(hoistPeers({
    autoInstallPeers: true,
    allPreferredVersions: {},
    workspaceRootDeps,
  }, [['postcss', { range: '^8.0.0' }]])).toStrictEqual({
    postcss: '8.5.10',
  })
  expect(getHoistableOptionalPeers({ postcss: ['*'] }, {
    postcss: {
      '8.5.10': 'version',
      '9.0.0': 'version',
    },
  }, workspaceRootDeps)).toStrictEqual({
    postcss: '8.5.10',
  })
})

test('getHoistableOptionalPeers stays within the version range of a scheme-prefixed workspace root specifier', () => {
  const allMissingOptionalPeers = { postcss: ['*'] }
  const allPreferredVersions = {
    postcss: {
      '8.5.10': 'version' as const,
      '9.0.0': 'version' as const,
    },
  }
  for (const normalizedBareSpecifier of ['workspace:^8.5.10', 'npm:postcss@^8.5.10', 'work:^8.5.10']) {
    expect(getHoistableOptionalPeers(allMissingOptionalPeers, allPreferredVersions, [
      { alias: 'postcss', pkgName: 'postcss', normalizedBareSpecifier },
    ])).toStrictEqual({
      postcss: '8.5.10',
    })
  }
})

test('getHoistableOptionalPeers keeps candidates unbounded when the workspace root specifier has no version', () => {
  expect(getHoistableOptionalPeers({ postcss: ['*'] }, {
    postcss: {
      '8.5.10': 'version',
      '9.0.0': 'version',
    },
  }, [
    { alias: 'postcss', pkgName: 'postcss', normalizedBareSpecifier: 'file:../postcss' },
  ])).toStrictEqual({
    postcss: '9.0.0',
  })
})
