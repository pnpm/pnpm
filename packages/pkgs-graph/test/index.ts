import test = require('tape')
import createPkgGraph from 'pkgs-graph'
import path = require('path')

test('create package graph', t => {
  const result = createPkgGraph([
    {
      manifest: {
        name: 'bar',
        version: '1.0.0',
        dependencies: {
          'is-positive': '1.0.0',
          foo: '^1.0.0'
        }
      },
      path: '/zkochan/src/bar',
    },
    {
      manifest: {
        name: 'foo',
        version: '1.0.0',
        dependencies: {
          bar: '^10.0.0'
        }
      },
      path: '/zkochan/src/foo',
    },
    {
      manifest: {
        name: 'bar',
        version: '2.0.0',
        dependencies: {
          foo: '^2.0.0'
        }
      },
      path: '/zkochan/src/bar@2',
    },
    {
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
      path: '/zkochan/src/foo@2',
    },
  ])
  t.deepEqual(result.unmatched, [{pkgName: 'bar', range: '^10.0.0'}])
  t.deepEqual(result.graph, {
    '/zkochan/src/bar': {
      package: {
        manifest: {
          name: 'bar',
          version: '1.0.0',
          dependencies: {
            'is-positive': '1.0.0',
            foo: '^1.0.0'
          }
        },
        path: '/zkochan/src/bar',
      },
      dependencies: ['/zkochan/src/foo'],
    },
    '/zkochan/src/foo': {
      package: {
        manifest: {
          name: 'foo',
          version: '1.0.0',
          dependencies: {
            bar: '^10.0.0'
          }
        },
        path: '/zkochan/src/foo',
      },
      dependencies: [],
    },
    '/zkochan/src/bar@2': {
      package: {
        manifest: {
          name: 'bar',
          version: '2.0.0',
          dependencies: {
            foo: '^2.0.0'
          }
        },
        path: '/zkochan/src/bar@2',
      },
      dependencies: ['/zkochan/src/foo@2'],
    },
    '/zkochan/src/foo@2': {
      package: {
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
        path: '/zkochan/src/foo@2',
      },
      dependencies: [],
    },
  })
  t.end()
})

test('create package graph for local directory dependencies', t => {
  const result = createPkgGraph([
    {
      manifest: {
        name: 'bar',
        version: '1.0.0',
        dependencies: {
          'weird-dep': ':aaaaa', // weird deps are skipped
          'is-positive': '1.0.0',
          foo: '../foo'
        }
      },
      path: '/zkochan/src/bar',
    },
    {
      manifest: {
        name: 'foo',
        version: '1.0.0',
        dependencies: {
          bar: '^10.0.0'
        }
      },
      path: '/zkochan/src/foo',
    },
    {
      manifest: {
        name: 'bar',
        version: '2.0.0',
        dependencies: {
          foo: 'file:../foo@2'
        }
      },
      path: '/zkochan/src/bar@2',
    },
    {
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
      path: '/zkochan/src/foo@2',
    },
  ])
  t.deepEqual(result.unmatched, [{pkgName: 'bar', range: '^10.0.0'}])
  t.deepEqual(result.graph, {
    '/zkochan/src/bar': {
      package: {
        manifest: {
          name: 'bar',
          version: '1.0.0',
          dependencies: {
            'weird-dep': ':aaaaa',
            'is-positive': '1.0.0',
            foo: '../foo'
          }
        },
        path: '/zkochan/src/bar',
      },
      dependencies: ['/zkochan/src/foo'],
    },
    '/zkochan/src/foo': {
      package: {
        manifest: {
          name: 'foo',
          version: '1.0.0',
          dependencies: {
            bar: '^10.0.0'
          }
        },
        path: '/zkochan/src/foo',
      },
      dependencies: [],
    },
    '/zkochan/src/bar@2': {
      package: {
        manifest: {
          name: 'bar',
          version: '2.0.0',
          dependencies: {
            foo: 'file:../foo@2'
          },
        },
        path: '/zkochan/src/bar@2',
      },
      dependencies: ['/zkochan/src/foo@2'],
    },
    '/zkochan/src/foo@2': {
      package: {
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
        path: '/zkochan/src/foo@2',
      },
      dependencies: [],
    },
  })
  t.end()
})
