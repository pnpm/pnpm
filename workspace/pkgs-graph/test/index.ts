/// <reference path="../../../__typings__/local.d.ts"/>
import { createPkgGraph } from 'pkgs-graph'
import pathResolve from 'better-path-resolve'

const BAR1_PATH = pathResolve('/zkochan/src/bar')
const FOO1_PATH = pathResolve('/zkochan/src/foo')
const BAR2_PATH = pathResolve('/zkochan/src/bar@2')
const FOO2_PATH = pathResolve('/zkochan/src/foo@2')
const BAR3_PATH = pathResolve('/zkochan/src/bar@3')
const BAR4_PATH = pathResolve('/zkochan/src/bar@4')
const BAR5_PATH = pathResolve('/zkochan/src/bar@5')

test('create package graph', () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          foo: '^1.0.0',
          'is-positive': '1.0.0',
        },
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      dir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: '^2.0.0',
        },
      },
    },
    {
      dir: FOO2_PATH,
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([{ pkgName: 'bar', range: '^10.0.0' }])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR1_PATH,
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            foo: '^1.0.0',
            'is-positive': '1.0.0',
          },
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0',

          dependencies: {
            bar: '^10.0.0',
          },
        },
      },
    },
    [BAR2_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        dir: BAR2_PATH,
        manifest: {
          name: 'bar',
          version: '2.0.0',

          dependencies: {
            foo: '^2.0.0',
          },
        },
      },
    },
    [FOO2_PATH]: {
      dependencies: [],
      package: {
        dir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
  })
})

test('create package graph for local directory dependencies', () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          foo: '../foo',
          'is-positive': '1.0.0',
          'weird-dep': ':aaaaa', // weird deps are skipped
        },
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      dir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: 'file:../foo@2',
        },
      },
    },
    {
      dir: FOO2_PATH,
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([{ pkgName: 'bar', range: '^10.0.0' }])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR1_PATH,
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            foo: '../foo',
            'is-positive': '1.0.0',
            'weird-dep': ':aaaaa',
          },
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0',

          dependencies: {
            bar: '^10.0.0',
          },
        },
      },
    },
    [BAR2_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        dir: BAR2_PATH,
        manifest: {
          name: 'bar',
          version: '2.0.0',

          dependencies: {
            foo: 'file:../foo@2',
          },
        },
      },
    },
    [FOO2_PATH]: {
      dependencies: [],
      package: {
        dir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
  })
})

test('create package graph ignoring the workspace protocol', () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          foo: 'workspace:^1.0.0',
          'is-positive': '1.0.0',
        },
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      dir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: 'workspace:^2.0.0',
        },
      },
    },
    {
      dir: FOO2_PATH,
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
    },
    {
      dir: BAR3_PATH,
      manifest: {
        name: 'bar',
        version: '3.0.0',

        dependencies: {
          foo: 'workspace:^',
        },
      },
    },
    {
      dir: BAR4_PATH,
      manifest: {
        name: 'bar',
        version: '4.0.0',

        dependencies: {
          foo: 'workspace:~',
        },
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([{ pkgName: 'bar', range: '^10.0.0' }])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR1_PATH,
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            foo: 'workspace:^1.0.0',
            'is-positive': '1.0.0',
          },
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0',

          dependencies: {
            bar: '^10.0.0',
          },
        },
      },
    },
    [BAR2_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        dir: BAR2_PATH,
        manifest: {
          name: 'bar',
          version: '2.0.0',

          dependencies: {
            foo: 'workspace:^2.0.0',
          },
        },
      },
    },
    [FOO2_PATH]: {
      dependencies: [],
      package: {
        dir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
    [BAR3_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        dir: BAR3_PATH,
        manifest: {
          name: 'bar',
          version: '3.0.0',

          dependencies: {
            foo: 'workspace:^',
          },
        },
      },
    },
    [BAR4_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        dir: BAR4_PATH,
        manifest: {
          name: 'bar',
          version: '4.0.0',

          dependencies: {
            foo: 'workspace:~',
          },
        },
      },
    },
  })
})

test('create package graph respects linked-workspace-packages = false', () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        dependencies: {
          bar: '^10.0.0',
        },
        name: 'foo',
        version: '1.0.1',
      },
    },
    {
      dir: BAR2_PATH,
      manifest: {
        dependencies: {
          foo: '1.0.1',
        },
        name: 'bar',
        version: '2.0.0',
      },
    },
    {
      dir: BAR3_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:~1.0.0',
        },
        name: 'bar',
        version: '3.0.0',
      },
    },
    {
      dir: BAR4_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:^',
        },
        name: 'bar',
        version: '4.0.0',
      },
    },
    {
      dir: BAR5_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:~',
        },
        name: 'bar',
        version: '5.0.0',
      },
    },
  ], { linkWorkspacePackages: false })
  expect(result.unmatched).toStrictEqual([{ pkgName: 'bar', range: '^10.0.0' }, { pkgName: 'foo', range: '1.0.1' }])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR1_PATH,
        manifest: {
          dependencies: {
            foo: 'workspace:*',
          },
          name: 'bar',
          version: '1.0.0',
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          dependencies: {
            bar: '^10.0.0',
          },
          name: 'foo',
          version: '1.0.1',
        },
      },
    },
    [BAR2_PATH]: {
      // no workspace range, so this shouldn't have any
      // workspace dependencies
      dependencies: [],
      package: {
        dir: BAR2_PATH,
        manifest: {
          dependencies: {
            foo: '1.0.1',
          },
          name: 'bar',
          version: '2.0.0',
        },
      },
    },
    [BAR3_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR3_PATH,
        manifest: {
          dependencies: {
            foo: 'workspace:~1.0.0',
          },
          name: 'bar',
          version: '3.0.0',
        },
      },
    },
    [BAR4_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR4_PATH,
        manifest: {
          dependencies: {
            foo: 'workspace:^',
          },
          name: 'bar',
          version: '4.0.0',
        },
      },
    },
    [BAR5_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR5_PATH,
        manifest: {
          dependencies: {
            foo: 'workspace:~',
          },
          name: 'bar',
          version: '5.0.0',
        },
      },
    },
  })
})

test('create package graph respects ignoreDevDeps = true', () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          foo: '^1.0.0',
        },
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      dir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: '^2.0.0',
        },
      },
    },
    {
      dir: FOO2_PATH,
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
    },
  ], { ignoreDevDeps: true })
  expect(result.unmatched).toStrictEqual([{ pkgName: 'bar', range: '^10.0.0' }])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [],
      package: {
        dir: BAR1_PATH,
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            'is-positive': '1.0.0',
          },
          devDependencies: {
            foo: '^1.0.0',
          },
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0',

          dependencies: {
            bar: '^10.0.0',
          },
        },
      },
    },
    [BAR2_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        dir: BAR2_PATH,
        manifest: {
          name: 'bar',
          version: '2.0.0',

          dependencies: {
            foo: '^2.0.0',
          },
        },
      },
    },
    [FOO2_PATH]: {
      dependencies: [],
      package: {
        dir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
  })
})

test('* matches prerelease versions', () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        dependencies: {
          foo: '*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0-0',
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR1_PATH,
        manifest: {
          dependencies: {
            foo: '*',
          },
          name: 'bar',
          version: '1.0.0',
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0-0',
        },
      },
    },
  })
})

// fix: https://github.com/pnpm/pnpm/issues/3933
test('successfully create a package graph even when a workspace package has no version', async () => {
  const result = createPkgGraph([
    {
      dir: BAR1_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      dir: FOO1_PATH,
      manifest: {
        name: 'foo',
      },
    },
  ])

  expect(result.unmatched).toStrictEqual([])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        dir: BAR1_PATH,
        manifest: {
          dependencies: {
            foo: 'workspace:*',
          },
          name: 'bar',
          version: '1.0.0',
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        dir: FOO1_PATH,
        manifest: {
          name: 'foo',
        },
      },
    },
  })
})
