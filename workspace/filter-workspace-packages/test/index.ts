import { promisify } from 'util'
import { type PnpmError } from '@pnpm/error'
import { filterWorkspacePackages, type PackageGraph } from '@pnpm/filter-workspace-packages'
import './parsePackageSelector'
import fs from 'fs'
import execa from 'execa'
import { isCI } from 'ci-info'
import isWindows from 'is-windows'
import path from 'path'
import omit from 'ramda/src/omit'
import tempy from 'tempy'
import touchCB from 'touch'

const touch = promisify(touchCB)
const mkdir = promisify(fs.mkdir)

const PKGS_GRAPH: PackageGraph<unknown> = {
  '/packages/project-0': {
    dependencies: ['/packages/project-1', '/project-5'],
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
  '/project-5': {
    dependencies: [],
    package: {
      dir: '/project-5',
      manifest: {
        name: 'project-5',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
  },
  '/project-5/packages/project-6': {
    dependencies: [],
    package: {
      dir: '/project-5/packages/project-6',
      manifest: {
        name: 'project-6',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
  },
}

test('select only package dependencies (excluding the package itself)', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-2', '/project-4'])
})

test('select package with dependencies', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      includeDependencies: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/project-1', '/project-2', '/project-4'])
})

test('select package with dependencies and dependents, including dependent dependencies', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      includeDependents: true,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-2', '/project-4', '/packages/project-0', '/packages/project-1', '/project-5'])
})

test('select package with dependents', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      includeDependents: true,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-2', '/packages/project-1', '/packages/project-0'])
})

test('select dependents excluding package itself', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependents: true,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/project-1', '/packages/project-0'])
})

test('filter using two selectors: one selects dependencies another selects dependents', async () => {
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

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-2', '/project-4', '/packages/project-1', '/packages/project-0'])
})

test('select just a package by name', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      namePattern: 'project-2',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-2'])
})

test('select package without specifying its scope', async () => {
  const PKGS_GRAPH: PackageGraph<unknown> = {
    '/packages/bar': {
      dependencies: [],
      package: {
        dir: '/packages/bar',
        manifest: {
          name: '@foo/bar',
          version: '1.0.0',
        },
      },
    },
  }
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      namePattern: 'bar',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/bar'])
})

test('when a scoped package with the same name exists, only pick the exact match', async () => {
  const PKGS_GRAPH: PackageGraph<unknown> = {
    '/packages/@foo/bar': {
      dependencies: [],
      package: {
        dir: '/packages/@foo/bar',
        manifest: {
          name: '@foo/bar',
          version: '1.0.0',
        },
      },
    },
    '/packages/bar': {
      dependencies: [],
      package: {
        dir: '/packages/bar',
        manifest: {
          name: 'bar',
          version: '1.0.0',
        },
      },
    },
  }
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      namePattern: 'bar',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/bar'])
})

test('when two scoped packages match the searched name, don\'t select any', async () => {
  const PKGS_GRAPH: PackageGraph<unknown> = {
    '/packages/@foo/bar': {
      dependencies: [],
      package: {
        dir: '/packages/@foo/bar',
        manifest: {
          name: '@foo/bar',
          version: '1.0.0',
        },
      },
    },
    '/packages/@types/bar': {
      dependencies: [],
      package: {
        dir: '/packages/@types/bar',
        manifest: {
          name: '@types/bar',
          version: '1.0.0',
        },
      },
    },
  }
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      namePattern: 'bar',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual([])
})

test('select by parentDir', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/packages',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/project-0', '/packages/project-1'])
})

test('select by parentDir using glob', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/packages/*',
    },
  ], { workspaceDir: process.cwd(), useGlobDirFiltering: true })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/project-0', '/packages/project-1'])
})

test('select by parentDir using globstar', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/project-5/**',
    },
  ], { workspaceDir: process.cwd(), useGlobDirFiltering: true })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-5', '/project-5/packages/project-6'])
})

test('select by parentDir with no glob', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/project-5',
    },
  ], { workspaceDir: process.cwd(), useGlobDirFiltering: true })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/project-5'])
})

test('select changed packages', async () => {
  // This test fails on Appveyor due to environmental issues
  if (isCI && isWindows()) {
    return
  }

  const workspaceDir = tempy.directory()
  await execa('git', ['init', '--initial-branch=main'], { cwd: workspaceDir })
  await execa('git', ['config', 'user.email', 'x@y.z'], { cwd: workspaceDir })
  await execa('git', ['config', 'user.name', 'xyz'], { cwd: workspaceDir })
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'], { cwd: workspaceDir })

  const pkg1Dir = path.join(workspaceDir, 'package-1')

  await mkdir(pkg1Dir)
  await touch(path.join(pkg1Dir, 'file1.js'))

  const pkg2Dir = path.join(workspaceDir, 'package-2')

  await mkdir(pkg2Dir)
  await touch(path.join(pkg2Dir, 'file2.js'))

  const pkg3Dir = path.join(workspaceDir, 'package-3')

  await mkdir(pkg3Dir)

  const pkgKorDir = path.join(workspaceDir, 'package-kor')

  await mkdir(pkgKorDir)
  await touch(path.join(pkgKorDir, 'fileKor한글.js'))

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
    [pkg3Dir]: {
      dependencies: [pkg2Dir],
      package: {
        dir: pkg3Dir,
        manifest: {
          name: 'package-3',
          version: '0.0.0',
        },
      },
    },
    [pkgKorDir]: {
      dependencies: [],
      package: {
        dir: pkgKorDir,
        manifest: {
          name: 'package-kor',
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

    expect(Object.keys(selectedProjectsGraph)).toStrictEqual([pkg1Dir, pkg2Dir, pkgKorDir])
  }
  {
    const { selectedProjectsGraph } = await filterWorkspacePackages(pkgsGraph, [{
      diff: 'HEAD~1',
      parentDir: pkg2Dir,
    }], { workspaceDir })

    expect(Object.keys(selectedProjectsGraph)).toStrictEqual([pkg2Dir])
  }
  {
    const { selectedProjectsGraph } = await filterWorkspacePackages(pkgsGraph, [{
      diff: 'HEAD~1',
      namePattern: 'package-2*',
    }], { workspaceDir })

    expect(Object.keys(selectedProjectsGraph)).toStrictEqual([pkg2Dir])
  }
  {
    const { selectedProjectsGraph } = await filterWorkspacePackages(pkgsGraph, [{
      diff: 'HEAD~1',
      includeDependents: true,
    }], { workspaceDir, testPattern: ['*/file2.js'] })

    expect(Object.keys(selectedProjectsGraph)).toStrictEqual([pkg1Dir, pkgKorDir, pkg2Dir])
  }
})

test('selection should fail when diffing to a branch that does not exist', async () => {
  let err!: PnpmError
  try {
    await filterWorkspacePackages(PKGS_GRAPH, [{ diff: 'branch-does-no-exist' }], { workspaceDir: process.cwd() })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err).toBeDefined()
  expect(err.code).toEqual('ERR_PNPM_FILTER_CHANGED')
  expect(err.message).toEqual("Filtering by changed packages failed. fatal: bad revision 'branch-does-no-exist'")
})

test('should return unmatched filters', async () => {
  const { unmatchedFilters } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: true,
      includeDependencies: true,
      namePattern: 'project-7',
    },
  ], { workspaceDir: process.cwd() })

  expect(unmatchedFilters).toStrictEqual(['project-7'])
})

test('select all packages except one', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      exclude: true,
      excludeSelf: false,
      includeDependencies: false,
      namePattern: 'project-1',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph))
    .toStrictEqual(Object.keys(omit(['/packages/project-1'], PKGS_GRAPH)))
})

test('select by parentDir and exclude one package by pattern', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/packages',
    },
    {
      exclude: true,
      excludeSelf: false,
      includeDependents: false,
      namePattern: '*-1',
    },
  ], { workspaceDir: process.cwd() })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/project-0'])
})

test('select by parentDir with glob and exclude one package by pattern', async () => {
  const { selectedProjectsGraph } = await filterWorkspacePackages(PKGS_GRAPH, [
    {
      excludeSelf: false,
      parentDir: '/packages/*',
    },
    {
      exclude: true,
      excludeSelf: false,
      includeDependents: false,
      namePattern: '*-1',
    },
  ], { workspaceDir: process.cwd(), useGlobDirFiltering: true })

  expect(Object.keys(selectedProjectsGraph)).toStrictEqual(['/packages/project-0'])
})
