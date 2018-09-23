import test = require('tape')
import dh, {forPackages as dhForPackages} from 'dependencies-hierarchy'
import path = require('path')

const fixtures = path.join(__dirname, 'fixtures')
const generalFixture = path.join(fixtures, 'general')
const circularFixture = path.join(fixtures, 'circular')
const withFileDepFixture = path.join(fixtures, 'with-file-dep')
const withLinksOnlyFixture = path.join(fixtures, 'with-links-only')

test('one package depth 0', async t => {
  const tree = await dh(generalFixture, {depth: 0})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'minimatch',
          path: 'registry.npmjs.org/minimatch/3.0.4',
          version: '3.0.4',
        }
      },
      {
        pkg: {
          name: 'rimraf',
          path: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
      },
      {
        pkg: {
          name: 'is-positive',
          path: 'registry.npmjs.org/is-positive/1.0.0',
          version: '1.0.0',
        }
      },
      {
        pkg: {
          name: 'is-negative',
          path: 'registry.npmjs.org/is-negative/1.0.0',
          version: '1.0.0',
        }
      },
  ])

  t.end()
})

test('one package depth 1', async t => {
  const tree = await dh(generalFixture, {depth: 1})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'minimatch',
          path: 'registry.npmjs.org/minimatch/3.0.4',
          version: '3.0.4',
        },
        dependencies: [
          {
            pkg: {
              name: 'brace-expansion',
              path: 'registry.npmjs.org/brace-expansion/1.1.8',
              version: '1.1.8',
            }
          }
        ],
      },
      {
        pkg: {
          name: 'rimraf',
          path: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
        dependencies: [
          {
            pkg: {
              name: 'glob',
              path: 'registry.npmjs.org/glob/6.0.4',
              version: '6.0.4',
            }
          }
        ]
      },
      {
        pkg: {
          name: 'is-positive',
          path: 'registry.npmjs.org/is-positive/1.0.0',
          version: '1.0.0',
        }
      },
      {
        pkg: {
          name: 'is-negative',
          path: 'registry.npmjs.org/is-negative/1.0.0',
          version: '1.0.0',
        }
      },
  ])

  t.end()
})

test('only prod depth 0', async t => {
  const tree = await dh(generalFixture, {depth: 0, only: 'prod'})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'minimatch',
          path: 'registry.npmjs.org/minimatch/3.0.4',
          version: '3.0.4',
        },
      },
      {
        pkg: {
          name: 'rimraf',
          path: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
      },
  ])

  t.end()
})

test('only dev depth 0', async t => {
  const tree = await dh(generalFixture, {depth: 0, only: 'dev'})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'is-positive',
          path: 'registry.npmjs.org/is-positive/1.0.0',
          version: '1.0.0',
        }
      },
  ])

  t.end()
})

test('hierarchy for no packages', async t => {
  const tree = await dhForPackages([], generalFixture, {depth: 100})

  t.deepEqual(tree, [])

  t.end()
})

test('filter 1 package with depth 0', async t => {
  const tree = await dhForPackages([{name: 'rimraf', range: '*'}], generalFixture, {depth: 0})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'rimraf',
          path: 'registry.npmjs.org/rimraf/2.5.1',
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
    {name: 'once', range: '1.4'},
  ]
  const tree = await dhForPackages(searched, generalFixture, {depth: 100})

  t.deepEqual(tree, [
    {
      pkg: {
        name: 'minimatch',
        path: 'registry.npmjs.org/minimatch/3.0.4',
        version: '3.0.4',
      },
      searched: true,
    },
    {
      pkg: {
        name: 'rimraf',
        path: 'registry.npmjs.org/rimraf/2.5.1',
        version: '2.5.1',
      },
      dependencies: [
        {
          pkg: {
            name: 'glob',
            path: 'registry.npmjs.org/glob/6.0.4',
            version: '6.0.4',
          },
          dependencies: [
            {
              pkg: {
                name: 'inflight',
                path: 'registry.npmjs.org/inflight/1.0.6',
                version: '1.0.6',
              },
              dependencies: [
                {
                  pkg: {
                    name: 'once',
                    path: 'registry.npmjs.org/once/1.4.0',
                    version: '1.4.0',
                  },
                  searched: true,
                }
              ]
            },
            {
              pkg: {
                name: 'minimatch',
                path: 'registry.npmjs.org/minimatch/3.0.4',
                version: '3.0.4',
              },
              searched: true,
            },
            {
              pkg: {
                name: 'once',
                path: 'registry.npmjs.org/once/1.4.0',
                version: '1.4.0',
              },
              searched: true,
            }
          ]
        }
      ]
    }
  ])

  t.end()
})

test('filter 2 packages with ranges that are not satisfied', async t => {
  const searched = [
    {name: 'minimatch', range: '100'},
    {name: 'once', range: '100'},
  ]
  const tree = await dhForPackages(searched, generalFixture, {depth: 100})

  t.deepEqual(tree, [])

  t.end()
})

test('circular dependency', async t => {
  const tree = await dh(circularFixture, {depth: 1000})

  t.deepEqual(tree, require('./circularTree.json'))

  t.end()
})

test('local package depth 0', async t => {
  const tree = await dh(withFileDepFixture, {depth: 1})

  t.deepEqual(tree, [
    {
      pkg: { name: 'general', path: 'file:../general', version: 'file:../general' }
    },
    {
      pkg: { name: 'is-positive', path: 'registry.npmjs.org/is-positive/3.1.0', version: '3.1.0' }
    },
  ])

  t.end()
})

test('on a package that has only links', async t => {
  const tree = await dh(withLinksOnlyFixture, {depth: 1000})

  t.deepEqual(tree, [
    {
      pkg: {
        name: 'general',
        path: 'link:../general',
        version: 'link:../general',
      },
    },
  ])

  t.end()
})
