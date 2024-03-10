/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { fixtures } from '@pnpm/test-fixtures'
import { buildDependenciesHierarchy, type PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { depPathToFilename } from '@pnpm/dependency-path'

const f = fixtures(__dirname)
const generalFixture = f.find('general')
const withPeerFixture = f.find('with-peer')
const circularFixture = f.find('circular')
const withFileDepFixture = f.find('with-file-dep')
const withNonPackageDepFixture = f.find('with-non-package-dep')
const withLinksOnlyFixture = f.find('fixtureWithLinks/with-links-only')
const withUnsavedDepsFixture = f.find('with-unsaved-deps')
const fixtureMonorepo = path.join(__dirname, '..', 'fixtureMonorepo')
const withAliasedDepFixture = f.find('with-aliased-dep')
const workspaceWithNestedWorkspaceDeps = f.find('workspace-with-nested-workspace-deps')
const customModulesDirFixture = f.find('custom-modules-dir')

test('one package depth 0', async () => {
  const tree = await buildDependenciesHierarchy([generalFixture], { depth: 0, lockfileDir: generalFixture })
  const modulesDir = path.join(generalFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [generalFixture]: {
      dependencies: [
        {
          alias: 'minimatch',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'minimatch',
          path: path.join(modulesDir, '.pnpm/minimatch@3.0.4/node_modules/minimatch'),
          resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
          version: '3.0.4',
        },
        {
          alias: 'rimraf',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'rimraf',
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1/node_modules/rimraf'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          version: '2.5.1',
        },
      ],
      devDependencies: [
        {
          alias: 'is-positive',
          dev: true,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-positive',
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
      optionalDependencies: [
        {
          alias: 'is-negative',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-negative',
          optional: true,
          path: path.join(modulesDir, '.pnpm/is-negative@1.0.0/node_modules/is-negative'),
          resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })
})

test('one package depth 1', async () => {
  const tree = await buildDependenciesHierarchy([generalFixture], { depth: 1, lockfileDir: generalFixture })
  const modulesDir = path.join(generalFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [generalFixture]: {
      dependencies: [
        {
          alias: 'minimatch',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'minimatch',
          path: path.join(modulesDir, '.pnpm/minimatch@3.0.4/node_modules/minimatch'),
          resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
          version: '3.0.4',

          dependencies: [
            {
              alias: 'brace-expansion',
              dev: false,
              isMissing: false,
              isPeer: false,
              isSkipped: false,
              name: 'brace-expansion',
              path: path.join(modulesDir, '.pnpm/brace-expansion@1.1.8/node_modules/brace-expansion'),
              resolved: 'https://registry.npmjs.org/brace-expansion/-/brace-expansion-1.1.8.tgz',
              version: '1.1.8',
            },
          ],
        },
        {
          alias: 'rimraf',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'rimraf',
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1/node_modules/rimraf'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          version: '2.5.1',

          dependencies: [
            {
              alias: 'glob',
              dev: false,
              isMissing: false,
              isPeer: false,
              isSkipped: false,
              name: 'glob',
              path: path.join(modulesDir, '.pnpm/glob@6.0.4/node_modules/glob'),
              resolved: 'https://registry.npmjs.org/glob/-/glob-6.0.4.tgz',
              version: '6.0.4',
            },
          ],
        },
      ],
      devDependencies: [
        {
          alias: 'is-positive',
          dev: true,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-positive',
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
      optionalDependencies: [
        {
          alias: 'is-negative',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-negative',
          optional: true,
          path: path.join(modulesDir, '.pnpm/is-negative@1.0.0/node_modules/is-negative'),
          resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })
})

test('only prod depth 0', async () => {
  const tree = await buildDependenciesHierarchy(
    [generalFixture],
    {
      depth: 0,
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: false,
      },
      lockfileDir: generalFixture,
    }
  )
  const modulesDir = path.join(generalFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [generalFixture]: {
      dependencies: [
        {
          alias: 'minimatch',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'minimatch',
          path: path.join(modulesDir, '.pnpm/minimatch@3.0.4/node_modules/minimatch'),
          resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
          version: '3.0.4',
        },
        {
          alias: 'rimraf',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'rimraf',
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1/node_modules/rimraf'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          version: '2.5.1',
        },
      ],
    },
  })
})

test('only dev depth 0', async () => {
  const tree = await buildDependenciesHierarchy(
    [generalFixture],
    {
      depth: 0,
      include: {
        dependencies: false,
        devDependencies: true,
        optionalDependencies: false,
      },
      lockfileDir: generalFixture,
    }
  )
  const modulesDir = path.join(generalFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [generalFixture]: {
      devDependencies: [
        {
          alias: 'is-positive',
          dev: true,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-positive',
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })
})

test('hierarchy for no packages', async () => {
  const tree = await buildDependenciesHierarchy([generalFixture], {
    depth: 100,
    lockfileDir: generalFixture,
    search: () => false,
  })

  expect(tree).toStrictEqual({
    [generalFixture]: {
      dependencies: [],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

test('filter 1 package with depth 0', async () => {
  const tree = await buildDependenciesHierarchy(
    [generalFixture],
    {
      depth: 0,
      lockfileDir: generalFixture,
      search: ({ name }) => name === 'rimraf',
    }
  )
  const modulesDir = path.join(generalFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [generalFixture]: {
      dependencies: [
        {
          alias: 'rimraf',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'rimraf',
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1/node_modules/rimraf'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          searched: true,
          version: '2.5.1',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

test('circular dependency', async () => {
  const tree = await buildDependenciesHierarchy([circularFixture], { depth: 1000, lockfileDir: circularFixture })
  const modulesDir = path.join(circularFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [circularFixture]: {
      dependencies: require('./circularTree.json') // eslint-disable-line
        .dependencies
        .map((dep: PackageNode) => resolvePaths(modulesDir, dep)),
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

function resolvePaths (modulesDir: string, node: PackageNode): PackageNode {
  const p = path.resolve(modulesDir, '.pnpm', node.path, 'node_modules', node.name)
  if (node.dependencies == null) {
    return {
      ...node,
      alias: node.name,
      path: p,
    }
  }
  return {
    ...node,
    alias: node.name,
    dependencies: node.dependencies.map((dep) => resolvePaths(modulesDir, dep)),
    path: p,
  }
}

test('local package depth 0', async () => {
  const tree = await buildDependenciesHierarchy([withFileDepFixture], { depth: 1, lockfileDir: withFileDepFixture })
  const modulesDir = path.join(withFileDepFixture, 'node_modules')

  expect(tree).toStrictEqual({
    [withFileDepFixture]: {
      dependencies: [
        {
          alias: 'general',
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'general',
          path: generalFixture,
          version: 'link:../general',
        },
        {
          alias: 'is-positive',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-positive',
          path: path.join(modulesDir, '.pnpm/is-positive@3.1.0/node_modules/is-positive'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
          version: '3.1.0',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

test('on a package that has only links', async () => {
  const tree = await buildDependenciesHierarchy([withLinksOnlyFixture], { depth: 1000, lockfileDir: withLinksOnlyFixture })

  expect(tree).toStrictEqual({
    [withLinksOnlyFixture]: {
      dependencies: [
        {
          alias: 'general',
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'general',
          path: path.join(f.find('fixtureWithLinks'), 'general'),
          version: 'link:../general',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

// Test for feature request at https://github.com/pnpm/pnpm/issues/4154
test('on a package with nested workspace links', async () => {
  const tree = await buildDependenciesHierarchy(
    [workspaceWithNestedWorkspaceDeps],
    { depth: 1000, lockfileDir: workspaceWithNestedWorkspaceDeps }
  )

  expect(tree).toEqual({
    [workspaceWithNestedWorkspaceDeps]: {
      dependencies: [
        expect.objectContaining({
          alias: '@scope/a',
          version: 'link:packages/a',
          path: path.join(workspaceWithNestedWorkspaceDeps, 'packages/a'),
          dependencies: [
            expect.objectContaining({
              alias: '@scope/b',
              version: 'link:packages/b',
              path: path.join(workspaceWithNestedWorkspaceDeps, 'packages/b'),
              dependencies: [
                expect.objectContaining({
                  alias: '@scope/c',
                  version: 'link:packages/c',
                  path: path.join(workspaceWithNestedWorkspaceDeps, 'packages/c'),
                }),
                expect.objectContaining({
                  alias: 'is-positive',
                  version: '1.0.0',
                }),
              ],
            }),
          ],
        }),
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

test('unsaved dependencies are listed', async () => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  expect(await buildDependenciesHierarchy([withUnsavedDepsFixture], { depth: 0, lockfileDir: withUnsavedDepsFixture }))
    .toStrictEqual({
      [withUnsavedDepsFixture]: {
        dependencies: [
          {
            alias: 'symlink-dir',
            dev: false,
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'symlink-dir',
            path: path.join(modulesDir, '.pnpm/symlink-dir@2.0.2/node_modules/symlink-dir'),
            resolved: 'https://registry.npmjs.org/symlink-dir/-/symlink-dir-2.0.2.tgz',
            version: '2.0.2',
          },
        ],
        devDependencies: [],
        optionalDependencies: [],
        unsavedDependencies: [
          {
            alias: 'general',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'general',
            path: generalFixture,
            version: 'link:../general',
          },
        ],
      },
    })
})

test('unsaved dependencies are listed and filtered', async () => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  expect(
    await buildDependenciesHierarchy(
      [withUnsavedDepsFixture],
      {
        depth: 0,
        lockfileDir: withUnsavedDepsFixture,
        search: ({ name }) => name === 'symlink-dir',
      }
    )
  ).toStrictEqual({
    [withUnsavedDepsFixture]: {
      dependencies: [
        {
          alias: 'symlink-dir',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'symlink-dir',
          path: path.join(modulesDir, '.pnpm/symlink-dir@2.0.2/node_modules/symlink-dir'),
          resolved: 'https://registry.npmjs.org/symlink-dir/-/symlink-dir-2.0.2.tgz',
          searched: true,
          version: '2.0.2',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

// Covers https://github.com/pnpm/pnpm/issues/1549
test(`do not fail on importers that are not in current ${WANTED_LOCKFILE}`, async () => {
  expect(await buildDependenciesHierarchy([fixtureMonorepo], { depth: 0, lockfileDir: fixtureMonorepo })).toStrictEqual({ [fixtureMonorepo]: {} })
})

test('dependency with an alias', async () => {
  const modulesDir = path.join(withAliasedDepFixture, 'node_modules')
  expect(
    await buildDependenciesHierarchy([withAliasedDepFixture], { depth: 0, lockfileDir: withAliasedDepFixture })
  ).toStrictEqual({
    [withAliasedDepFixture]: {
      dependencies: [
        {
          alias: 'positive',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-positive',
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

test('peer dependencies', async () => {
  const hierarchy = await buildDependenciesHierarchy([withPeerFixture], { depth: 1, lockfileDir: withPeerFixture })
  expect(hierarchy[withPeerFixture].dependencies![1].dependencies![0].name).toEqual('ajv')
  expect(hierarchy[withPeerFixture].dependencies![1].dependencies![0].isPeer).toEqual(true)
})

// Test case for https://github.com/pnpm/pnpm/issues/1866
test('dependency without a package.json', async () => {
  const org = 'denolib'
  const pkg = 'camelcase'
  const commit = 'aeb6b15f9c9957c8fa56f9731e914c4d8a6d2f2b'
  const tree = await buildDependenciesHierarchy([withNonPackageDepFixture], { depth: 0, lockfileDir: withNonPackageDepFixture })
  const resolved = `https://codeload.github.com/${org}/${pkg}/tar.gz/${commit}`
  expect(tree).toStrictEqual({
    [withNonPackageDepFixture]: {
      dependencies: [
        {
          alias: 'camelcase',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: `camelcase#${commit}`,
          path: path.join(withNonPackageDepFixture, 'node_modules', '.pnpm', depPathToFilename(resolved), 'node_modules', `camelcase#${commit}`),
          resolved,
          version: '0.0.0',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
  // verify dependency without a package.json
  expect(tree[withNonPackageDepFixture].dependencies).toBeDefined()
  expect(Array.isArray(tree[withNonPackageDepFixture].dependencies)).toBeTruthy()
  expect(tree[withNonPackageDepFixture].dependencies!.length).toBeGreaterThan(0)
  expect(tree[withNonPackageDepFixture].dependencies![0]).toBeDefined()
  // verify that dependency without a package.json has no further dependencies
  expect(tree[withNonPackageDepFixture].dependencies![0]).not.toHaveProperty(['dependencies'])
  expect(tree[withNonPackageDepFixture].dependencies![0]).not.toHaveProperty(['devDependencies'])
  expect(tree[withNonPackageDepFixture].dependencies![0]).not.toHaveProperty(['optionalDependencies'])
})

test('on custom modules-dir workspaces', async () => {
  const tree = await buildDependenciesHierarchy(
    [customModulesDirFixture, path.join(customModulesDirFixture, './packages/foo'), path.join(customModulesDirFixture, './packages/bar')],
    { depth: 1000, lockfileDir: customModulesDirFixture, modulesDir: 'fake_modules' }
  )
  expect(tree).toEqual({
    [customModulesDirFixture]: {
      dependencies: [],
      devDependencies: [],
      optionalDependencies: [],
    },
    [path.join(customModulesDirFixture, 'packages/foo')]: {
      dependencies: [
        expect.objectContaining({
          alias: '@scope/bar',
          version: 'link:../bar',
          path: path.join(customModulesDirFixture, 'packages/bar'),
          dependencies: [
            expect.objectContaining({
              alias: 'is-positive',
              name: 'is-positive',
              path: path.join(customModulesDirFixture, 'fake_modules/.fake_store/is-positive@1.0.0/node_modules/is-positive'),
              version: '1.0.0',
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'is-positive',
          name: 'is-positive',
          path: path.join(customModulesDirFixture, 'fake_modules/.fake_store/is-positive@3.1.0/node_modules/is-positive'),
          version: '3.1.0',
        }),
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
    [path.join(customModulesDirFixture, 'packages/bar')]: {
      dependencies: [
        expect.objectContaining({
          alias: 'is-positive',
          name: 'is-positive',
          path: path.join(customModulesDirFixture, 'fake_modules/.fake_store/is-positive@1.0.0/node_modules/is-positive'),
          version: '1.0.0',
        }),
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})
