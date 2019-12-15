import filterWorkspacePackages, { PackageGraph } from '@pnpm/filter-workspace-packages'
import test = require('tape')
import './parsePackageSelector'

const PKGS_GRAPH: PackageGraph<{}> = {
  '/packages/project-0': {
    dependencies: ['/packages/project-1'],
    package: {
      dir: '/packages/project-0',
      manifest: {
        name: 'project-0',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'project-1': '1.0.0',
        },
      },
    },
  },
  '/packages/project-1': {
    dependencies: ['/project-2', '/project-4'],
    package: {
      dir: '/packages/project-1',
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'project-2': '1.0.0',
          'project-4': '1.0.0',
        },
      },
    },
  },
  '/project-2': {
    dependencies: [],
    package: {
      dir: '/project-2',
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
  },
  '/project-3': {
    dependencies: [],
    package: {
      dir: '/project-3',
      manifest: {
        name: 'project-3',
        version: '1.0.0',

        dependencies: {
          minimatch: '*',
        },
      },
    },
  },
  '/project-4': {
    dependencies: [],
    package: {
      dir: '/project-4',
      manifest: {
        name: 'project-4',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
  },
}

test('select only package dependencies (excluding the package itself)', async (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      pattern: 'project-1',
      scope: 'dependencies',
      selectBy: 'name',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/project-2', '/project-4'])

  t.end()
})

test('select package with dependencies', async (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      pattern: 'project-1',
      scope: 'dependencies',
      selectBy: 'name',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/packages/project-1', '/project-2', '/project-4'])

  t.end()
})

test('select package with dependents', async (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      pattern: 'project-2',
      scope: 'dependents',
      selectBy: 'name',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/project-2', '/packages/project-1', '/packages/project-0'])

  t.end()
})

test('select dependents excluding package itself', async (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      pattern: 'project-2',
      scope: 'dependents',
      selectBy: 'name',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/packages/project-1', '/packages/project-0'])

  t.end()
})

test('filter using two selectors: one selects dependencies another selects dependents', async (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      pattern: 'project-2',
      scope: 'dependents',
      selectBy: 'name',
    },
    {
      excludeSelf: true,
      pattern: 'project-1',
      scope: 'dependencies',
      selectBy: 'name',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/project-2', '/project-4', '/packages/project-1', '/packages/project-0'])

  t.end()
})

test('select just a package by name', (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      pattern: 'project-2',
      scope: 'exact',
      selectBy: 'name',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/project-2'])

  t.end()
})

test('select by location', (t) => {
  const selection = filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      pattern: '/packages',
      scope: 'exact',
      selectBy: 'location',
    },
  ])

  t.deepEqual(Object.keys(selection), ['/packages/project-0', '/packages/project-1'])

  t.end()
})
