import test = require('tape')
import dh, {forPackages as dhForPackages} from '../src'
import path = require('path')

const fixture = path.join(__dirname, 'fixture')

test('one package depth 0', async t => {
  const tree = await dh(fixture, {depth: 0})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'minimatch',
          resolvedId: 'registry.npmjs.org/minimatch/3.0.4',
          version: '3.0.4',
        }
      },
      {
        pkg: {
          name: 'rimraf',
          resolvedId: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
      },
      {
        pkg: {
          name: 'is-positive',
          resolvedId: 'registry.npmjs.org/is-positive/1.0.0',
          version: '1.0.0',
        }
      },
      {
        pkg: {
          name: 'is-negative',
          resolvedId: 'registry.npmjs.org/is-negative/1.0.0',
          version: '1.0.0',
        }
      },
  ])

  t.end()
})

test('one package depth 1', async t => {
  const tree = await dh(fixture, {depth: 1})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'minimatch',
          resolvedId: 'registry.npmjs.org/minimatch/3.0.4',
          version: '3.0.4',
        },
        dependencies: [
          {
            pkg: {
              name: 'brace-expansion',
              resolvedId: 'registry.npmjs.org/brace-expansion/1.1.8',
              version: '1.1.8',
            }
          }
        ],
      },
      {
        pkg: {
          name: 'rimraf',
          resolvedId: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
        dependencies: [
          {
            pkg: {
              name: 'glob',
              resolvedId: 'registry.npmjs.org/glob/6.0.4',
              version: '6.0.4',
            }
          }
        ]
      },
      {
        pkg: {
          name: 'is-positive',
          resolvedId: 'registry.npmjs.org/is-positive/1.0.0',
          version: '1.0.0',
        }
      },
      {
        pkg: {
          name: 'is-negative',
          resolvedId: 'registry.npmjs.org/is-negative/1.0.0',
          version: '1.0.0',
        }
      },
  ])

  t.end()
})

test('only prod depth 0', async t => {
  const tree = await dh(fixture, {depth: 0, only: 'prod'})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'minimatch',
          resolvedId: 'registry.npmjs.org/minimatch/3.0.4',
          version: '3.0.4',
        },
      },
      {
        pkg: {
          name: 'rimraf',
          resolvedId: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
      },
  ])

  t.end()
})

test('only dev depth 0', async t => {
  const tree = await dh(fixture, {depth: 0, only: 'dev'})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'is-positive',
          resolvedId: 'registry.npmjs.org/is-positive/1.0.0',
          version: '1.0.0',
        }
      },
  ])

  t.end()
})

test('hierarchy for no packages', async t => {
  const tree = await dhForPackages([], fixture, {depth: 100})

  t.deepEqual(tree, [])

  t.end()
})

test('filter 1 package with depth 0', async t => {
  const tree = await dhForPackages([{name: 'rimraf', range: '*'}], fixture, {depth: 0})

  t.deepEqual(tree, [
      {
        pkg: {
          name: 'rimraf',
          resolvedId: 'registry.npmjs.org/rimraf/2.5.1',
          version: '2.5.1',
        },
        searched: true,
      },
  ])

  t.end()
})

test('filter 2 packages with depth 100', async t => {
  const searched = [
    {name: 'minimatch', range: '*'},
    {name: 'once', range: '*'},
  ]
  const tree = await dhForPackages(searched, fixture, {depth: 100})

  t.deepEqual(tree, [
    {
      pkg: {
        name: 'minimatch',
        resolvedId: 'registry.npmjs.org/minimatch/3.0.4',
        version: '3.0.4',
      },
      searched: true,
    },
    {
      pkg: {
        name: 'rimraf',
        resolvedId: 'registry.npmjs.org/rimraf/2.5.1',
        version: '2.5.1',
      },
      dependencies: [
        {
          pkg: {
            name: 'glob',
            resolvedId: 'registry.npmjs.org/glob/6.0.4',
            version: '6.0.4',
          },
          dependencies: [
            {
              pkg: {
                name: 'inflight',
                resolvedId: 'registry.npmjs.org/inflight/1.0.6',
                version: '1.0.6',
              },
              dependencies: [
                {
                  pkg: {
                    name: 'once',
                    resolvedId: 'registry.npmjs.org/once/1.4.0',
                    version: '1.4.0',
                  },
                  searched: true,
                }
              ]
            },
            {
              pkg: {
                name: 'minimatch',
                resolvedId: 'registry.npmjs.org/minimatch/3.0.4',
                version: '3.0.4',
              },
              searched: true,
            },
            {
              pkg: {
                name: 'once',
                resolvedId: 'registry.npmjs.org/once/1.4.0',
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
