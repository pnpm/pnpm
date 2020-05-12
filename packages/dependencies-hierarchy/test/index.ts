///<reference path="../../../typings/index.d.ts"/>
import { WANTED_LOCKFILE } from '@pnpm/constants'
import dh, { PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import test = require('tape')

const fixtures = path.join(__dirname, '..', 'fixtures')
const generalFixture = path.join(fixtures, 'general')
const withPeerFixture = path.join(fixtures, 'with-peer')
const circularFixture = path.join(fixtures, 'circular')
const withFileDepFixture = path.join(fixtures, 'with-file-dep')
const withLinksOnlyFixture = path.join(__dirname, '..', 'fixtureWithLinks', 'with-links-only')
const withUnsavedDepsFixture = path.join(fixtures, 'with-unsaved-deps')
const fixtureMonorepo = path.join(__dirname, '..', 'fixtureMonorepo')
const withAliasedDepFixture = path.join(fixtures, 'with-aliased-dep')

test('one package depth 0', async t => {
  const tree = await dh([generalFixture], { depth: 0, lockfileDir: generalFixture })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, {
    [generalFixture]: {
      dependencies: [
        {
          alias: 'minimatch',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'minimatch',
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/minimatch/3.0.4'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/rimraf/2.5.1'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-positive/1.0.0'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-negative/1.0.0'),
          resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })

  t.end()
})

test('one package depth 1', async t => {
  const tree = await dh([generalFixture], { depth: 1, lockfileDir: generalFixture })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, {
    [generalFixture]: {
      dependencies: [
        {
          alias: 'minimatch',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'minimatch',
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/minimatch/3.0.4'),
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
              path: path.join(modulesDir, '.pnpm/registry.npmjs.org/brace-expansion/1.1.8'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/rimraf/2.5.1'),
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
              path: path.join(modulesDir, '.pnpm/registry.npmjs.org/glob/6.0.4'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-positive/1.0.0'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-negative/1.0.0'),
          resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })

  t.end()
})

test('only prod depth 0', async t => {
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

  t.deepEqual(tree, {
    [generalFixture]: {
      dependencies: [
        {
          alias: 'minimatch',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'minimatch',
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/minimatch/3.0.4'),
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/rimraf/2.5.1'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          version: '2.5.1',
        },
      ],
    },
  })

  t.end()
})

test('only dev depth 0', async t => {
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

  t.deepEqual(tree, {
    [generalFixture]: {
      devDependencies: [
        {
          alias: 'is-positive',
          dev: true,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'is-positive',
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-positive/1.0.0'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
          version: '1.0.0',
        },
      ],
    },
  })

  t.end()
})

test('hierarchy for no packages', async t => {
  const tree = await dh([generalFixture], {
    depth: 100,
    lockfileDir: generalFixture,
    search: () => false,
  })

  t.deepEqual(tree, {
    [generalFixture]: {
      dependencies: [],
      devDependencies: [],
      optionalDependencies: [],
    },
  })

  t.end()
})

test('filter 1 package with depth 0', async t => {
  const tree = await dh(
    [generalFixture],
    {
      depth: 0,
      lockfileDir: generalFixture,
      search: ({ name }) => name === 'rimraf',
    }
  )
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, {
    [generalFixture]: {
      dependencies: [
        {
          alias: 'rimraf',
          dev: false,
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'rimraf',
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/rimraf/2.5.1'),
          resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
          searched: true,
          version: '2.5.1',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })

  t.end()
})

test('circular dependency', async t => {
  const tree = await dh([circularFixture], { depth: 1000, lockfileDir: circularFixture })
  const modulesDir = path.join(circularFixture, 'node_modules')

  t.deepEqual(tree, {
    [circularFixture]: {
      dependencies: require('./circularTree.json')
        .dependencies
        .map((dep: PackageNode) => resolvePaths(modulesDir, dep)),
      devDependencies: [],
      optionalDependencies: [],
    },
  })

  t.end()
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

test('local package depth 0', async t => {
  const tree = await dh([withFileDepFixture], { depth: 1, lockfileDir: withFileDepFixture })
  const modulesDir = path.join(withFileDepFixture, 'node_modules')

  t.deepEqual(tree, {
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
          path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-positive/3.1.0'),
          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
          version: '3.1.0',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })

  t.end()
})

test('on a package that has only links', async t => {
  const tree = await dh([withLinksOnlyFixture], { depth: 1000, lockfileDir: withLinksOnlyFixture })

  t.deepEqual(tree, {
    [withLinksOnlyFixture]: {
      dependencies: [
        {
          alias: 'general',
          isMissing: false,
          isPeer: false,
          isSkipped: false,
          name: 'general',
          path: path.join(__dirname, '..', 'fixtureWithLinks', 'general'),
          version: 'link:../general',
        },
      ],
      devDependencies: [],
      optionalDependencies: [],
    },
  })

  t.end()
})

test('unsaved dependencies are listed', async t => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  t.deepEqual(
    await dh([withUnsavedDepsFixture], { depth: 0, lockfileDir: withUnsavedDepsFixture }),
    {
      [withUnsavedDepsFixture]: {
        dependencies: [
          {
            alias: 'symlink-dir',
            dev: false,
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'symlink-dir',
            path: path.join(modulesDir, '.pnpm/registry.npmjs.org/symlink-dir/2.0.2'),
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
          {
            alias: 'is-positive',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'is-positive',
            path: path.join(modulesDir, 'is-positive'),
            version: '3.1.0',
          },
        ],
      },
    }
  )
  t.end()
})

test('unsaved dependencies are listed and filtered', async t => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  t.deepEqual(
    await dh(
      [withUnsavedDepsFixture],
      {
        depth: 0,
        lockfileDir: withUnsavedDepsFixture,
        search: ({ name }) => name === 'symlink-dir',
      }
    ),
    {
      [withUnsavedDepsFixture]: {
        dependencies: [
          {
            alias: 'symlink-dir',
            dev: false,
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'symlink-dir',
            path: path.join(modulesDir, '.pnpm/registry.npmjs.org/symlink-dir/2.0.2'),
            resolved: 'https://registry.npmjs.org/symlink-dir/-/symlink-dir-2.0.2.tgz',
            searched: true,
            version: '2.0.2',
          },
        ],
        devDependencies: [],
        optionalDependencies: [],
      },
    }
  )
  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1549
test(`do not fail on importers that are not in current ${WANTED_LOCKFILE}`, async t => {
  t.deepEqual(await dh([fixtureMonorepo], { depth: 0, lockfileDir: fixtureMonorepo }), { [fixtureMonorepo]: {} })
  t.end()
})

test('dependency with an alias', async t => {
  const modulesDir = path.join(withAliasedDepFixture, 'node_modules')
  t.deepEqual(
    await dh([withAliasedDepFixture], { depth: 0, lockfileDir: withAliasedDepFixture }),
    {
      [withAliasedDepFixture]: {
        dependencies: [
          {
            alias: 'positive',
            dev: false,
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'is-positive',
            path: path.join(modulesDir, '.pnpm/registry.npmjs.org/is-positive/1.0.0'),
            resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
            version: '1.0.0',
          },
        ],
        devDependencies: [],
        optionalDependencies: [],
      },
    }
  )
  t.end()
})

test('peer dependencies', async t => {
  const hierarchy = await dh([withPeerFixture], { depth: 1, lockfileDir: withPeerFixture })
  t.equal(hierarchy[withPeerFixture].dependencies![1].dependencies![0].name, 'ajv')
  t.equal(hierarchy[withPeerFixture].dependencies![1].dependencies![0].isPeer, true)
  t.end()
})
