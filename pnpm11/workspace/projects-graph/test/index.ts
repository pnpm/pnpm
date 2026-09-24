/// <reference path="../../../__typings__/local.d.ts"/>
import { expect, test } from '@jest/globals'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'
import { betterPathResolve as pathResolve } from 'better-path-resolve'

const BAR1_PATH = pathResolve('/zkochan/src/bar')
const FOO1_PATH = pathResolve('/zkochan/src/foo')
const BAR2_PATH = pathResolve('/zkochan/src/bar@2')
const FOO2_PATH = pathResolve('/zkochan/src/foo@2')
const BAR3_PATH = pathResolve('/zkochan/src/bar@3')
const BAR4_PATH = pathResolve('/zkochan/src/bar@4')
const BAR5_PATH = pathResolve('/zkochan/src/bar@5')

test('create package graph', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
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
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: '^2.0.0',
        },
      },
    },
    {
      rootDir: FOO2_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
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
        rootDir: BAR2_PATH,
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
        rootDir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
  })
})

test('create package graph using peer dependencies', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        peerDependencies: {
          foo: '^1.0.0',
          'is-positive': '1.0.0',
        },
      },
    },
    {
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        rootDir: BAR1_PATH,
        manifest: {
          name: 'bar',
          version: '1.0.0',

          peerDependencies: {
            foo: '^1.0.0',
            'is-positive': '1.0.0',
          },
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        rootDir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0',
        },
      },
    },
  })
})

test('create package graph for local directory dependencies', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
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
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: 'file:../foo@2',
        },
      },
    },
    {
      rootDir: FOO2_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
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
        rootDir: BAR2_PATH,
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
        rootDir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
  })
})

test('create package graph for local directory dependencies using the workspace protocol', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          foo: 'workspace:../foo',
        },
      },
    },
    {
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([])
  expect(result.graph).toStrictEqual({
    [BAR1_PATH]: {
      dependencies: [FOO1_PATH],
      package: {
        rootDir: BAR1_PATH,
        manifest: {
          name: 'bar',
          version: '1.0.0',

          dependencies: {
            foo: 'workspace:../foo',
          },
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        rootDir: FOO1_PATH,
        manifest: {
          name: 'foo',
          version: '1.0.0',
        },
      },
    },
  })
})

test('create package graph for local directory dependencies using the workspace protocol with a ./ prefix', () => {
  const NESTED_FOO_PATH = pathResolve('/zkochan/src/bar/nested-foo')
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',

        dependencies: {
          foo: 'workspace:./nested-foo',
        },
      },
    },
    {
      rootDir: NESTED_FOO_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
    },
  ])
  expect(result.unmatched).toStrictEqual([])
  expect(result.graph[BAR1_PATH].dependencies).toStrictEqual([NESTED_FOO_PATH])
})

test('create package graph ignoring the workspace protocol', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
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
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: 'workspace:^2.0.0',
        },
      },
    },
    {
      rootDir: FOO2_PATH,
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
    },
    {
      rootDir: BAR3_PATH,
      manifest: {
        name: 'bar',
        version: '3.0.0',

        dependencies: {
          foo: 'workspace:^',
        },
      },
    },
    {
      rootDir: BAR4_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
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
        rootDir: BAR2_PATH,
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
        rootDir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
    [BAR3_PATH]: {
      dependencies: [FOO2_PATH],
      package: {
        rootDir: BAR3_PATH,
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
        rootDir: BAR4_PATH,
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
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      rootDir: FOO1_PATH,
      manifest: {
        dependencies: {
          bar: '^10.0.0',
        },
        name: 'foo',
        version: '1.0.1',
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        dependencies: {
          foo: '1.0.1',
        },
        name: 'bar',
        version: '2.0.0',
      },
    },
    {
      rootDir: BAR3_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:~1.0.0',
        },
        name: 'bar',
        version: '3.0.0',
      },
    },
    {
      rootDir: BAR4_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:^',
        },
        name: 'bar',
        version: '4.0.0',
      },
    },
    {
      rootDir: BAR5_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
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
        rootDir: BAR2_PATH,
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
        rootDir: BAR3_PATH,
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
        rootDir: BAR4_PATH,
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
        rootDir: BAR5_PATH,
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
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
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
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.0.0',

        dependencies: {
          bar: '^10.0.0',
        },
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        name: 'bar',
        version: '2.0.0',

        dependencies: {
          foo: '^2.0.0',
        },
      },
    },
    {
      rootDir: FOO2_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
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
        rootDir: BAR2_PATH,
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
        rootDir: FOO2_PATH,
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
      },
    },
  })
})

test('* matches prerelease versions', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        dependencies: {
          foo: '*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      rootDir: FOO1_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
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
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        dependencies: {
          foo: 'workspace:*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      rootDir: FOO1_PATH,
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
        rootDir: BAR1_PATH,
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
        rootDir: FOO1_PATH,
        manifest: {
          name: 'foo',
        },
      },
    },
  })
})

test('create package graph respects workspace alias syntax', async () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        dependencies: {
          'foo-alias': 'workspace:foo@*',
        },
        name: 'bar',
        version: '1.0.0',
      },
    },
    {
      rootDir: FOO1_PATH,
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
        rootDir: BAR1_PATH,
        manifest: {
          dependencies: {
            'foo-alias': 'workspace:foo@*',
          },
          name: 'bar',
          version: '1.0.0',
        },
      },
    },
    [FOO1_PATH]: {
      dependencies: [],
      package: {
        rootDir: FOO1_PATH,
        manifest: {
          name: 'foo',
        },
      },
    },
  })
})

test('create graph with dependencies resolved through catalogs', () => {
  const result = createProjectsGraph([
    {
      rootDir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',
        dependencies: {
          foo: 'catalog:',
          'is-positive': 'catalog:',
        },
        devDependencies: {
          baz: 'catalog:tools',
        },
      },
    },
    {
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        name: 'baz',
        version: '2.0.0',
      },
    },
  ], {
    catalogs: {
      default: {
        foo: 'workspace:*',
        'is-positive': '1.0.0',
      },
      tools: {
        baz: '^2.0.0',
      },
    },
  })
  expect(result.unmatched).toStrictEqual([])
  expect(result.graph[BAR1_PATH].dependencies.sort()).toStrictEqual([BAR2_PATH, FOO1_PATH].sort())
})

test('create graph with dependencies declared through npm aliases', () => {
  const projects = [
    {
      rootDir: BAR1_PATH,
      manifest: {
        name: 'bar',
        version: '1.0.0',
        dependencies: {
          'foo-alias': 'npm:foo@^1.0.0',
          'baz-alias': 'npm:baz@^9.0.0',
          'is-positive-alias': 'npm:is-positive@1.0.0',
          qar: 'npm:^4.0.0',
          'foo-latest': 'npm:foo',
        },
      },
    },
    {
      rootDir: FOO1_PATH,
      manifest: {
        name: 'foo',
        version: '1.2.0',
      },
    },
    {
      rootDir: BAR2_PATH,
      manifest: {
        name: 'baz',
        version: '2.0.0',
      },
    },
    {
      rootDir: FOO2_PATH,
      manifest: {
        name: 'qar',
        version: '4.0.0',
      },
    },
  ]
  const linked = createProjectsGraph(projects)
  expect(linked.graph[BAR1_PATH].dependencies).toStrictEqual([FOO1_PATH, FOO2_PATH])
  expect(linked.unmatched).toStrictEqual([{ pkgName: 'baz', range: '>=9.0.0 <10.0.0-0' }])

  const strict = createProjectsGraph(projects, { linkWorkspacePackages: false })
  expect(strict.graph[BAR1_PATH].dependencies).toStrictEqual([])
  expect(strict.unmatched).toStrictEqual([
    { pkgName: 'foo', range: '>=1.0.0 <2.0.0-0' },
    { pkgName: 'baz', range: '>=9.0.0 <10.0.0-0' },
    { pkgName: 'qar', range: '>=4.0.0 <5.0.0-0' },
  ])
})
