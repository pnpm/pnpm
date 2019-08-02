///<reference path="../../../typings/index.d.ts"/>
import { WANTED_LOCKFILE } from '@pnpm/constants'
import dh, { forPackages as dhForPackages, PackageNode } from 'dependencies-hierarchy'
import path = require('path')
import test = require('tape')

const fixtures = path.join(__dirname, '..', 'fixtures')
const generalFixture = path.join(fixtures, 'general')
const circularFixture = path.join(fixtures, 'circular')
const withFileDepFixture = path.join(fixtures, 'with-file-dep')
const withLinksOnlyFixture = path.join(__dirname, '..', 'fixtureWithLinks', 'with-links-only')
const withUnsavedDepsFixture = path.join(fixtures, 'with-unsaved-deps')
const fixtureMonorepo = path.join(__dirname, '..', 'fixtureMonorepo')
const withAliasedDepFixture = path.join(fixtures, 'with-aliased-dep')

test('one package depth 0', async t => {
  const tree = await dh(generalFixture, { depth: 0 })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'minimatch',
        name: 'minimatch',
        path: path.join(modulesDir, '.registry.npmjs.org/minimatch/3.0.4'),
        resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
        version: '3.0.4',
      },
    },
    {
      pkg: {
        alias: 'rimraf',
        name: 'rimraf',
        path: path.join(modulesDir, '.registry.npmjs.org/rimraf/2.5.1'),
        resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
        version: '2.5.1',
      },
    },
    {
      pkg: {
        alias: 'is-positive',
        name: 'is-positive',
        path: path.join(modulesDir, '.registry.npmjs.org/is-positive/1.0.0'),
        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        version: '1.0.0',
      },
    },
    {
      pkg: {
        alias: 'is-negative',
        name: 'is-negative',
        path: path.join(modulesDir, '.registry.npmjs.org/is-negative/1.0.0'),
        resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
        version: '1.0.0',
      },
    },
  ])

  t.end()
})

test('one package depth 1', async t => {
  const tree = await dh(generalFixture, { depth: 1 })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      dependencies: [
        {
          pkg: {
            alias: 'brace-expansion',
            name: 'brace-expansion',
            path: path.join(modulesDir, '.registry.npmjs.org/brace-expansion/1.1.8'),
            resolved: 'https://registry.npmjs.org/brace-expansion/-/brace-expansion-1.1.8.tgz',
            version: '1.1.8',
          },
        },
      ],
      pkg: {
        alias: 'minimatch',
        name: 'minimatch',
        path: path.join(modulesDir, '.registry.npmjs.org/minimatch/3.0.4'),
        resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
        version: '3.0.4',
      },
    },
    {
      dependencies: [
        {
          pkg: {
            alias: 'glob',
            name: 'glob',
            path: path.join(modulesDir, '.registry.npmjs.org/glob/6.0.4'),
            resolved: 'https://registry.npmjs.org/glob/-/glob-6.0.4.tgz',
            version: '6.0.4',
          },
        },
      ],
      pkg: {
        alias: 'rimraf',
        name: 'rimraf',
        path: path.join(modulesDir, '.registry.npmjs.org/rimraf/2.5.1'),
        resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
        version: '2.5.1',
      },
    },
    {
      pkg: {
        alias: 'is-positive',
        name: 'is-positive',
        path: path.join(modulesDir, '.registry.npmjs.org/is-positive/1.0.0'),
        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        version: '1.0.0',
      },
    },
    {
      pkg: {
        alias: 'is-negative',
        name: 'is-negative',
        path: path.join(modulesDir, '.registry.npmjs.org/is-negative/1.0.0'),
        resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-1.0.0.tgz',
        version: '1.0.0',
      },
    },
  ])

  t.end()
})

test('only prod depth 0', async t => {
  const tree = await dh(generalFixture, { depth: 0, only: 'prod' })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'minimatch',
        name: 'minimatch',
        path: path.join(modulesDir, '.registry.npmjs.org/minimatch/3.0.4'),
        resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
        version: '3.0.4',
      },
    },
    {
      pkg: {
        alias: 'rimraf',
        name: 'rimraf',
        path: path.join(modulesDir, '.registry.npmjs.org/rimraf/2.5.1'),
        resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
        version: '2.5.1',
      },
    },
  ])

  t.end()
})

test('only dev depth 0', async t => {
  const tree = await dh(generalFixture, { depth: 0, only: 'dev' })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'is-positive',
        name: 'is-positive',
        path: path.join(modulesDir, '.registry.npmjs.org/is-positive/1.0.0'),
        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        version: '1.0.0',
      }
    },
  ])

  t.end()
})

test('hierarchy for no packages', async t => {
  const tree = await dhForPackages([], generalFixture, { depth: 100 })

  t.deepEqual(tree, [])

  t.end()
})

test('filter 1 package with depth 0', async t => {
  const tree = await dhForPackages([{ name: 'rimraf', range: '*' }], generalFixture, { depth: 0 })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'rimraf',
        name: 'rimraf',
        path: path.join(modulesDir, '.registry.npmjs.org/rimraf/2.5.1'),
        resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
        version: '2.5.1',
      },
      searched: true,
    },
  ])

  t.end()
})

test('filter 2 packages with depth 100', async t => {
  const searched = [
    'minimatch',
    { name: 'once', range: '1.4' },
  ]
  const tree = await dhForPackages(searched, generalFixture, { depth: 100 })
  const modulesDir = path.join(generalFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'minimatch',
        name: 'minimatch',
        path: path.join(modulesDir, '.registry.npmjs.org/minimatch/3.0.4'),
        resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
        version: '3.0.4',
      },
      searched: true,
    },
    {
      dependencies: [
        {
          dependencies: [
            {
              dependencies: [
                {
                  pkg: {
                    alias: 'once',
                    name: 'once',
                    path: path.join(modulesDir, '.registry.npmjs.org/once/1.4.0'),
                    resolved: 'https://registry.npmjs.org/once/-/once-1.4.0.tgz',
                    version: '1.4.0',
                  },
                  searched: true,
                },
              ],
              pkg: {
                alias: 'inflight',
                name: 'inflight',
                path: path.join(modulesDir, '.registry.npmjs.org/inflight/1.0.6'),
                resolved: 'https://registry.npmjs.org/inflight/-/inflight-1.0.6.tgz',
                version: '1.0.6',
              },
            },
            {
              pkg: {
                alias: 'minimatch',
                name: 'minimatch',
                path: path.join(modulesDir, '.registry.npmjs.org/minimatch/3.0.4'),
                resolved: 'https://registry.npmjs.org/minimatch/-/minimatch-3.0.4.tgz',
                version: '3.0.4',
              },
              searched: true,
            },
            {
              pkg: {
                alias: 'once',
                name: 'once',
                path: path.join(modulesDir, '.registry.npmjs.org/once/1.4.0'),
                resolved: 'https://registry.npmjs.org/once/-/once-1.4.0.tgz',
                version: '1.4.0',
              },
              searched: true,
            },
          ],
          pkg: {
            alias: 'glob',
            name: 'glob',
            path: path.join(modulesDir, '.registry.npmjs.org/glob/6.0.4'),
            resolved: 'https://registry.npmjs.org/glob/-/glob-6.0.4.tgz',
            version: '6.0.4',
          },
        },
      ],
      pkg: {
        alias: 'rimraf',
        name: 'rimraf',
        path: path.join(modulesDir, '.registry.npmjs.org/rimraf/2.5.1'),
        resolved: 'https://registry.npmjs.org/rimraf/-/rimraf-2.5.1.tgz',
        version: '2.5.1',
      },
    }
  ])

  t.end()
})

test('filter 2 packages with ranges that are not satisfied', async t => {
  const searched = [
    { name: 'minimatch', range: '100' },
    { name: 'once', range: '100' },
  ]
  const tree = await dhForPackages(searched, generalFixture, { depth: 100 })

  t.deepEqual(tree, [])

  t.end()
})

test('circular dependency', async t => {
  const tree = await dh(circularFixture, { depth: 1000 })
  const modulesDir = path.join(circularFixture, 'node_modules')

  t.deepEqual(tree, require('./circularTree.json').map((dep: PackageNode) => resolvePaths(modulesDir, dep)))

  t.end()
})

function resolvePaths (modulesDir: string, node: PackageNode): PackageNode {
  const p = path.resolve(modulesDir, `.${node.pkg.path}`)
  if (!node.dependencies) {
    return {
      ...node,
      pkg: {
        ...node.pkg,
        alias: node.pkg.name,
        path: p,
      },
    }
  }
  return {
    ...node,
    dependencies: node.dependencies.map((dep) => resolvePaths(modulesDir, dep)),
    pkg: {
      ...node.pkg,
      alias: node.pkg.name,
      path: p,
    },
  }
}

test('local package depth 0', async t => {
  const tree = await dh(withFileDepFixture, { depth: 1 })
  const modulesDir = path.join(withFileDepFixture, 'node_modules')

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'general',
        name: 'general',
        path: generalFixture,
        version: 'link:../general',
      },
    },
    {
      pkg: {
        alias: 'is-positive',
        name: 'is-positive',
        path: path.join(modulesDir, '.registry.npmjs.org/is-positive/3.1.0'),
        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
        version: '3.1.0',
      },
    },
  ])

  t.end()
})

test('on a package that has only links', async t => {
  const tree = await dh(withLinksOnlyFixture, { depth: 1000 })

  t.deepEqual(tree, [
    {
      pkg: {
        alias: 'general',
        name: 'general',
        path: path.join(__dirname, '..', 'fixtureWithLinks', 'general'),
        version: 'link:../general',
      },
    },
  ])

  t.end()
})

test('filter on a package that has only links', async t => {
  t.deepEqual(await dhForPackages(['rimraf'], withLinksOnlyFixture, { depth: 1000 }), [], 'not found')
  t.deepEqual(await dhForPackages([{ name: 'general', range: '2' }], withLinksOnlyFixture, { depth: 1000 }), [], 'not found')
  t.deepEqual(await dhForPackages(['general'], withLinksOnlyFixture, { depth: 1000 }), [
    {
      pkg: {
        alias: 'general',
        name: 'general',
        path: path.join(__dirname, '..', 'fixtureWithLinks', 'general'),
        version: 'link:../general',
      },
      searched: true,
    },
  ], 'found')

  t.end()
})

test('unsaved dependencies are listed', async t => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  t.deepEqual(await dh(withUnsavedDepsFixture), [
    {
      pkg: {
        alias: 'symlink-dir',
        name: 'symlink-dir',
        path: path.join(modulesDir, '.registry.npmjs.org/symlink-dir/2.0.2'),
        resolved: 'https://registry.npmjs.org/symlink-dir/-/symlink-dir-2.0.2.tgz',
        version: '2.0.2',
      },
    },
    {
      pkg: {
        alias: 'general',
        name: 'general',
        path: generalFixture,
        version: 'link:../general',
      },
      saved: false,
    },
    {
      pkg: {
        alias: 'is-positive',
        name: 'is-positive',
        path: path.join(modulesDir, 'is-positive'),
        version: '3.1.0',
      },
      saved: false,
    },
  ])
  t.end()
})

test('unsaved dependencies are listed and filtered', async t => {
  const modulesDir = path.join(withUnsavedDepsFixture, 'node_modules')
  t.deepEqual(await dhForPackages([{ name: 'symlink-dir', range: '*' }], withUnsavedDepsFixture), [
    {
      pkg: {
        alias: 'symlink-dir',
        name: 'symlink-dir',
        path: path.join(modulesDir, '.registry.npmjs.org/symlink-dir/2.0.2'),
        resolved: 'https://registry.npmjs.org/symlink-dir/-/symlink-dir-2.0.2.tgz',
        version: '2.0.2',
      },
      searched: true,
    },
  ])
  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1549
test(`do not fail on importers that are not in current ${WANTED_LOCKFILE}`, async t => {
  t.deepEqual(await dh(fixtureMonorepo), [])
  t.end()
})

test('dependency with an alias', async t => {
  const modulesDir = path.join(withAliasedDepFixture, 'node_modules')
  t.deepEqual(await dh(withAliasedDepFixture), [
    {
      pkg: {
        alias: 'positive',
        name: 'is-positive',
        path: path.join(modulesDir, '.registry.npmjs.org/is-positive/1.0.0'),
        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        version: '1.0.0',
      },
    },
  ])
  t.end()
})
