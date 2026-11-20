import { expect, test } from '@jest/globals'
import type { PkgResolutionId } from '@pnpm/types'

import { addDirectDependenciesToLockfile } from '../lib/addDirectDependenciesToLockfile.js'
import type { ResolvedDirectDependency } from '../lib/resolveDependencyTree.js'

test.each([
  ['catalog:', 'default'],
  ['catalog:named', 'named'],
])('keeps a workspace dependency resolved from %s when links are excluded', (specifier, catalogName) => {
  const dependency: ResolvedDirectDependency & { isLinkedDependency: true } = {
    alias: 'workspace-dep',
    catalogLookup: {
      catalogName,
      specifier: 'workspace:*',
      userSpecifiedBareSpecifier: specifier,
    },
    dev: false,
    isLinkedDependency: true,
    name: 'workspace-dep',
    optional: false,
    pkgId: 'link:../workspace-dep' as PkgResolutionId,
    resolution: { type: 'directory', directory: '../workspace-dep' },
    version: '1.0.0',
  }
  const projectSnapshot = addDirectDependenciesToLockfile({
    directDependencies: [dependency],
    excludeLinksFromLockfile: true,
    linkedPackages: [],
    newManifest: { dependencies: { 'workspace-dep': specifier } },
    projectSnapshot: { specifiers: {} },
  })

  expect(projectSnapshot).toStrictEqual({
    dependencies: { 'workspace-dep': 'link:../workspace-dep' },
    devDependencies: {},
    optionalDependencies: {},
    specifiers: { 'workspace-dep': specifier },
  })
})

test.each(['dependencies', 'devDependencies', 'optionalDependencies'] as const)(
  'drops an excluded resolved catalog link from %s while preserving unresolved entries',
  (dependencyField) => {
    const dependency: ResolvedDirectDependency & { isLinkedDependency: true } = {
      alias: 'workspace-dep',
      catalogLookup: {
        catalogName: 'default',
        specifier: '^1.0.0',
        userSpecifiedBareSpecifier: 'catalog:',
      },
      dev: dependencyField === 'devDependencies',
      isLinkedDependency: true,
      name: 'workspace-dep',
      optional: dependencyField === 'optionalDependencies',
      pkgId: 'link:../new-workspace-dep' as PkgResolutionId,
      resolution: { type: 'directory', directory: '../new-workspace-dep' },
      version: '1.0.0',
    }
    const specifiers = { 'workspace-dep': 'catalog:', 'unresolved-dep': '^1.0.0' }
    const projectSnapshot = addDirectDependenciesToLockfile({
      directDependencies: [dependency],
      excludeLinksFromLockfile: true,
      linkedPackages: [{ alias: 'workspace-dep' }],
      newManifest: { [dependencyField]: specifiers },
      projectSnapshot: {
        [dependencyField]: {
          'workspace-dep': 'link:../old-workspace-dep',
          'unresolved-dep': '1.0.0',
        },
        specifiers,
      },
    })

    expect(projectSnapshot).toStrictEqual({
      dependencies: {},
      devDependencies: {},
      optionalDependencies: {},
      [dependencyField]: { 'unresolved-dep': '1.0.0' },
      specifiers,
    })
  }
)

test('addDirectDependenciesToLockfile() locks a dependency whose alias is an Object.prototype key', () => {
  const projectSnapshot = addDirectDependenciesToLockfile({
    directDependencies: [{
      alias: 'constructor',
      dev: false,
      name: 'constructor',
      optional: false,
      pkgId: 'constructor@1.0.0' as PkgResolutionId,
      resolution: { integrity: 'sha512-AAAA', tarball: 'https://registry.npmjs.org/constructor/-/constructor-1.0.0.tgz' },
      version: '1.0.0',
    }],
    linkedPackages: [],
    newManifest: {
      dependencies: { constructor: '^1.0.0' },
    },
    projectSnapshot: { specifiers: {} },
  })
  expect(projectSnapshot).toStrictEqual({
    dependencies: { constructor: '1.0.0' },
    devDependencies: {},
    optionalDependencies: {},
    specifiers: { constructor: '^1.0.0' },
  })
})

test('addDirectDependenciesToLockfile() keeps the locked entry of an unresolved dependency whose alias is an Object.prototype key', () => {
  const projectSnapshot = addDirectDependenciesToLockfile({
    directDependencies: [],
    linkedPackages: [],
    newManifest: {
      dependencies: { constructor: '^1.0.0' },
    },
    projectSnapshot: {
      dependencies: { constructor: '1.0.0' },
      specifiers: { constructor: '^1.0.0' },
    },
  })
  expect(projectSnapshot).toStrictEqual({
    dependencies: { constructor: '1.0.0' },
    devDependencies: {},
    optionalDependencies: {},
    specifiers: { constructor: '^1.0.0' },
  })
})
