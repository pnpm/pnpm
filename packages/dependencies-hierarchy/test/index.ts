/// <reference path="../../../typings/index.d.ts"/>
import { WANTED_LOCKFILE } from '@pnpm/constants'
import dh, { PackageNode } from 'dependencies-hierarchy'
import path = require('path')

const fixtures = path.join(__dirname, '../../../fixtures')
const generalFixture = path.join(fixtures, 'general')
const withPeerFixture = path.join(fixtures, 'with-peer')
const circularFixture = path.join(fixtures, 'circular')
const withFileDepFixture = path.join(fixtures, 'with-file-dep')
const withLinksOnlyFixture = path.join(fixtures, 'fixtureWithLinks/with-links-only')
const withUnsavedDepsFixture = path.join(fixtures, 'with-unsaved-deps')
const fixtureMonorepo = path.join(__dirname, '..', 'fixtureMonorepo')
const withAliasedDepFixture = path.join(fixtures, 'with-aliased-dep')

test('one package depth 0', async () => {
  const tree = await dh([generalFixture], { depth: 0, lockfileDir: generalFixture })
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
          path: path.join(modulesDir, '.pnpm/minimatch@3.0.4'),
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
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1'),
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
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0'),
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
          path: path.join(modulesDir, '.pnpm/is-negative@1.0.0'),
          resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })
})

test('one package depth 1', async () => {
  const tree = await dh([generalFixture], { depth: 1, lockfileDir: generalFixture })
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
          path: path.join(modulesDir, '.pnpm/minimatch@3.0.4'),
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
              path: path.join(modulesDir, '.pnpm/brace-expansion@1.1.8'),
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
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1'),
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
              path: path.join(modulesDir, '.pnpm/glob@6.0.4'),
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
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0'),
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
          path: path.join(modulesDir, '.pnpm/is-negative@1.0.0'),
          resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })
})

test('only prod depth 0', async () => {
  const tree = await dh(
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
          path: path.join(modulesDir, '.pnpm/minimatch@3.0.4'),
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
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          version: '2.5.1',
        },
      ],
    },
  })
})

test('only dev depth 0', async () => {
  const tree = await dh(
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
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })
})

test('hierarchy for no packages', async () => {
  const tree = await dh([generalFixture], {
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
  const tree = await dh(
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
          path: path.join(modulesDir, '.pnpm/rimraf@2.5.1'),
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
  const tree = await dh([circularFixture], { depth: 1000, lockfileDir: circularFixture })
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
  const p = path.resolve(modulesDir, '.pnpm', node.path)
  if (!node.dependencies) {
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
  const tree = await dh([withFileDepFixture], { depth: 1, lockfileDir: withFileDepFixture })
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
          path: path.join(modulesDir, '.pnpm/is-positive@3.1.0'),
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
  const tree = await dh([withLinksOnlyFixture], { depth: 1000, lockfileDir: withLinksOnlyFixture })

  expect(tree).toStrictEqual({
    [withLinksOnlyFixture]: {
      dependencies: [
        {
          alias: 'general',
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'general',
          path: path.join(fixtures, 'fixtureWithLinks/general'),
          version: 'link:../general',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })
})

test('unsaved dependencies are listed', async () => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  expect(await dh([withUnsavedDepsFixture], { depth: 0, lockfileDir: withUnsavedDepsFixture }))
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
            path: path.join(modulesDir, '.pnpm/symlink-dir@2.0.2'),
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
    await dh(
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
          path: path.join(modulesDir, '.pnpm/symlink-dir@2.0.2'),
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
  expect(await dh([fixtureMonorepo], { depth: 0, lockfileDir: fixtureMonorepo })).toStrictEqual({ [fixtureMonorepo]: {} })
})

test('dependency with an alias', async () => {
  const modulesDir = path.join(withAliasedDepFixture, 'node_modules')
  expect(
    await dh([withAliasedDepFixture], { depth: 0, lockfileDir: withAliasedDepFixture })
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
          path: path.join(modulesDir, '.pnpm/is-positive@1.0.0'),
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
  const hierarchy = await dh([withPeerFixture], { depth: 1, lockfileDir: withPeerFixture })
  expect(hierarchy[withPeerFixture].dependencies![1].dependencies![0].name).toEqual('ajv')
  expect(hierarchy[withPeerFixture].dependencies![1].dependencies![0].isPeer).toEqual(true)
})
