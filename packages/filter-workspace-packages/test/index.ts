import { promisify } from 'util'
import PnpmError from '@pnpm/error'
import filterWorkspacePackages, { PackageGraph } from '@pnpm/filter-workspace-packages'
import './parsePackageSelector'
import fs = require('fs')
import execa = require('execa')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import touchCB = require('touch')

const touch = promisify(touchCB)
const mkdir = promisify(fs.mkdir)

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
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/project-2', '/project-4'])

  t.end()
})

test('select package with dependencies', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/packages/project-1', '/project-2', '/project-4'])

  t.end()
})

test('select package with dependencies and dependents', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      includeDependents: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/project-2', '/project-4', '/packages/project-0'])

  t.end()
})

test('select package with dependents', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      includeDependents: true,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/project-2', '/packages/project-1', '/packages/project-0'])

  t.end()
})

test('select dependents excluding package itself', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependents: true,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/packages/project-1', '/packages/project-0'])

  t.end()
})

test('filter using two selectors: one selects dependencies another selects dependents', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependents: true,
      namePattern: 'project-2',
    },
    {
      excludeSelf: true,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/project-2', '/project-4', '/packages/project-1', '/packages/project-0'])

  t.end()
})

test('select just a package by name', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/project-2'])

  t.end()
})

test('select by parentDir', async (t) => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/packages',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selectedProjectsGraph), ['/packages/project-0', '/packages/project-1'])

  t.end()
})

test('select changed packages', async (t) => {
  // This test fails on Appveyor due to environmental issues
  if (isCI && isWindows()) {
    t.end()
    return
  }
  const workspaceDir = tempy.directory()
  await execa('git', ['init'], { cwd: workspaceDir })
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'], { cwd: workspaceDir })

  const pkg1Dir = path.join(workspaceDir, 'package-1')

  await mkdir(pkg1Dir)
  await touch(path.join(pkg1Dir, 'file.js'))

  const pkg2Dir = path.join(workspaceDir, 'package-2')

  await mkdir(pkg2Dir)
  await touch(path.join(pkg2Dir, 'file.js'))

  await execa('git', ['add', '.'], { cwd: workspaceDir })
  await execa('git', ['commit', '--allow-empty-message', '-m', '', '--no-gpg-sign'], { cwd: workspaceDir })

  const pkg20Dir = path.join(workspaceDir, 'package-20')

  const pkgsGraph = {
    [workspaceDir]: {
      dependencies: [],
      package: {
        dir: workspaceDir,
        manifest: {
          name: 'root',
          version: '0.0.0',
        },
      },
    },
    [pkg1Dir]: {
      dependencies: [],
      package: {
        dir: pkg1Dir,
        manifest: {
          name: 'package-1',
          version: '0.0.0',
        },
      },
    },
    [pkg2Dir]: {
      dependencies: [],
      package: {
        dir: pkg2Dir,
        manifest: {
          name: 'package-2',
          version: '0.0.0',
        },
      },
    },
    [pkg20Dir]: {
      dependencies: [],
      package: {
        dir: pkg20Dir,
        manifest: {
          name: 'package-20',
          version: '0.0.0',
        },
      },
    },
  }

  {
    const { selectedProjectsGraph } = await filterWorkspacePackages(pkgsGraph, [{
      diff: 'HEAD~1',
    }], { workspaceDir })

    t.deepEqual(Object.keys(selectedProjectsGraph), [pkg1Dir, pkg2Dir])
  }
  {
    const { selectedProjectsGraph } = await filterWorkspacePackages(pkgsGraph, [{
      diff: 'HEAD~1',
      parentDir: pkg2Dir,
    }], { workspaceDir })

    t.deepEqual(Object.keys(selectedProjectsGraph), [pkg2Dir])
  }
  {
    const { selectedProjectsGraph } = await filterWorkspacePackages(pkgsGraph, [{
      diff: 'HEAD~1',
      namePattern: 'package-2*',
    }], { workspaceDir })

    t.deepEqual(Object.keys(selectedProjectsGraph), [pkg2Dir])
  }

  t.end()
})

test('selection should fail when diffing to a branch that does not exist', async (t) => {
  let err!: PnpmError
  try {
    await filterWorkspacePackages(PKGS_GRAPH, [{ diff: 'branch-does-no-exist' }], { workspaceDir: process.cwd() })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_FILTER_CHANGED')
  t.equal(err.message, "Filtering by changed packages failed. fatal: bad revision 'branch-does-no-exist'")
  t.end()
})

test('should return unmatched filters', async (t) => {
  const { unmatchedFilters } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      namePattern: 'project-5',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(unmatchedFilters, ['project-5'])

  t.end()
})
