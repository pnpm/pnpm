///<reference path="../../../typings/local.d.ts"/>
import pathResolve = require('better-path-resolve')
import createPkgGraph from 'pkgs-graph'
import test = require('tape')

const BAR1_PATH = pathResolve('/zkochan/src/bar')
const FOO1_PATH = pathResolve('/zkochan/src/foo')
const BAR2_PATH = pathResolve('/zkochan/src/bar@2')
const FOO2_PATH = pathResolve('/zkochan/src/foo@2')

test('create package graph', t => {
  const result = createPkgGraph([
    {
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          'foo': '^1.0.0',
          'is-positive': '1.0.0',
        }
      },
      path: BAR1_PATH,
    },
    {
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0'
        }
      },
      path: FOO1_PATH,
    },
    {
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: '^2.0.0'
        }
      },
      path: BAR2_PATH,
    },
    {
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
      path: FOO2_PATH,
    },
  ])
  t.deepEqual(result.unmatched, [{ pkgName: 'bar', range: '^10.0.0' }])
  t.deepEqual(result.graph, {
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            'foo': '^1.0.0',
            'is-positive': '1.0.0',
          }
        },
        path: BAR1_PATH,
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        manifest: {
          name: 'foo',
          version: '1.0.0',

          dependencies: {
            bar: '^10.0.0'
          }
        },
        path: FOO1_PATH,
      },
    },
    [BAR2_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        manifest: {
          name: 'bar',
          version: '2.0.0',

          dependencies: {
            foo: '^2.0.0',
          },
        },
        path: BAR2_PATH,
      },
    },
    [FOO2_PATH]: {
      dependencies: [],
      package: {
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
        path: FOO2_PATH,
      },
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
          'foo': '../foo',
          'is-positive': '1.0.0',
          'weird-dep': ':aaaaa', // weird deps are skipped
        },
      },
      path: BAR1_PATH,
    },
    {
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
      path: FOO1_PATH,
    },
    {
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: 'file:../foo@2',
        },
      },
      path: BAR2_PATH,
    },
    {
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
      path: FOO2_PATH,
    },
  ])
  t.deepEqual(result.unmatched, [{ pkgName: 'bar', range: '^10.0.0' }])
  t.deepEqual(result.graph, {
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            'foo': '../foo',
            'is-positive': '1.0.0',
            'weird-dep': ':aaaaa',
          },
        },
        path: BAR1_PATH,
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        manifest: {
          name: 'foo',
          version: '1.0.0',

          dependencies: {
            bar: '^10.0.0',
          },
        },
        path: FOO1_PATH,
      },
    },
    [BAR2_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        manifest: {
          name: 'bar',
          version: '2.0.0',

          dependencies: {
            foo: 'file:../foo@2'
          },
        },
        path: BAR2_PATH,
      },
    },
    [FOO2_PATH]: {
      dependencies: [],
      package: {
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
        path: FOO2_PATH,
      },
    },
  })
  t.end()
})
