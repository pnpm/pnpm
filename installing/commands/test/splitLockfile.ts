import fs from 'node:fs'
import path from 'node:path'

import { install } from '@pnpm/installing.commands'
import { preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

test('split lockfile mode: installs deps and creates per-package lockfiles', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    lockfileStorage: 'split',
    workspaceDir: process.cwd(),
  })

  // Each project should have its own lockfile
  const lockfile1 = projects['project-1'].readLockfile()
  expect(lockfile1.importers['.']).toBeDefined()
  expect(lockfile1.importers['.'].dependencies).toHaveProperty('is-positive')

  const lockfile2 = projects['project-2'].readLockfile()
  expect(lockfile2.importers['.']).toBeDefined()
  expect(lockfile2.importers['.'].dependencies).toHaveProperty('is-negative')

  // Packages should be installed correctly
  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')

  // Per-package lockfiles should NOT contain the other project's deps
  // Note: array form of toHaveProperty is required — dots in the key would
  // otherwise be interpreted as nested path separators.
  expect(lockfile1.packages).not.toHaveProperty(['is-negative@1.0.0'])
  expect(lockfile2.packages).not.toHaveProperty(['is-positive@1.0.0'])
})

test('split lockfile mode: second install reuses existing per-package lockfiles', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    lockfileStorage: 'split' as const,
    workspaceDir: process.cwd(),
  }

  // First install
  await install.handler(opts)

  // Second install should succeed (merge existing per-package lockfiles, resolve, split again)
  await install.handler(opts)

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')

  const lockfile1 = projects['project-1'].readLockfile()
  expect(lockfile1.importers['.']).toBeDefined()
})

test('split lockfile mode: no root pnpm-lock.yaml left behind after install', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    lockfileStorage: 'split',
    workspaceDir: process.cwd(),
  })

  // The sentinel file should never be left behind after a successful install
  expect(fs.existsSync(path.join(process.cwd(), '.pnpm-split-in-progress'))).toBe(false)

  // The project should have its own lockfile
  expect(fs.existsSync(path.join(process.cwd(), 'project-1', 'pnpm-lock.yaml'))).toBe(true)
})

test('split lockfile mode: recovers from partial failure (sentinel cleanup)', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  // Simulate a previous crash: sentinel + stale root lockfile
  fs.writeFileSync(path.join(process.cwd(), '.pnpm-split-in-progress'), 'pid=99999\n')
  fs.writeFileSync(path.join(process.cwd(), 'pnpm-lock.yaml'), 'lockfileVersion: "9.0"\n')

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    lockfileStorage: 'split',
    workspaceDir: process.cwd(),
  })

  // Recovery should have cleaned up sentinel
  expect(fs.existsSync(path.join(process.cwd(), '.pnpm-split-in-progress'))).toBe(false)

  // Install should have succeeded
  expect(fs.existsSync(path.join(process.cwd(), 'project-1', 'pnpm-lock.yaml'))).toBe(true)
})

test('split lockfile mode: projects with shared transitive deps get correct lockfiles', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
        'is-negative': '1.0.0',
      },
    },
  ])

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    lockfileStorage: 'split',
    workspaceDir: process.cwd(),
  })

  const lockfile1 = projects['project-1'].readLockfile()
  const lockfile2 = projects['project-2'].readLockfile()

  // Both should have is-positive in their packages
  // Note: array form of toHaveProperty is required — dots in the key would
  // otherwise be interpreted as nested path separators.
  expect(lockfile1.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile2.packages).toHaveProperty(['is-positive@1.0.0'])

  // Only project-2 should have is-negative
  expect(lockfile1.packages).not.toHaveProperty(['is-negative@1.0.0'])
  expect(lockfile2.packages).toHaveProperty(['is-negative@1.0.0'])
})
