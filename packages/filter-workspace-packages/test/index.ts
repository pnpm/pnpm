import filterWorkspacePackages, { PackageGraph } from '@pnpm/filter-workspace-packages'
import execa = require('execa')
import fs = require('fs')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import touchCB = require('touch')
import { promisify } from 'util'
import './parsePackageSelector'

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
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/project-2', '/project-4'])

  t.end()
})

test('select package with dependencies', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/packages/project-1', '/project-2', '/project-4'])

  t.end()
})

test('select package with dependencies and dependents', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      includeDependents: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/project-2', '/project-4', '/packages/project-0'])

  t.end()
})

test('select package with dependents', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      includeDependents: true,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/project-2', '/packages/project-1', '/packages/project-0'])

  t.end()
})

test('select dependents excluding package itself', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependents: true,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/packages/project-1', '/packages/project-0'])

  t.end()
})

test('filter using two selectors: one selects dependencies another selects dependents', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
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

  t.deepEqual(Object.keys(selection), ['/project-2', '/project-4', '/packages/project-1', '/packages/project-0'])

  t.end()
})

test('select just a package by name', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/project-2'])

  t.end()
})

test('select by parentDir', async (t) => {
  const selection = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/packages',
    },
  ], { workspaceDir: process.cwd() })

  t.deepEqual(Object.keys(selection), ['/packages/project-0', '/packages/project-1'])

  t.end()
})

test('select changed packages', async (t) => {
  // This test fails on Appveyor due to environmental issues
  if (isCI && isWindows()) return
  const workspaceDir = tempy.directory()
  await execa('git', ['init'], { cwd: workspaceDir })
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'], { cwd: workspaceDir })

  const pkg1Dir = path.join(workspaceDir, 'package-1')

  await mkdir(pkg1Dir)
  await touch(path.join(pkg1Dir, 'file.js'))

  await execa('git', ['add', '.'], { cwd: workspaceDir })
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'], { cwd: workspaceDir })

  const selection = await filterWorkspacePackages({
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
  }, [{
    diff: 'HEAD~1',
  }], { workspaceDir })

  t.deepEqual(Object.keys(selection), [pkg1Dir])

  t.end()
})
