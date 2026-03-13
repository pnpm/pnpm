import path from 'node:path'

import { assertProject } from '@pnpm/assert-project'
import { type MutatedProject, mutateModules, type ProjectOptions } from '@pnpm/core'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

test('workspace packages should maintain consistent protocol when injectWorkspacePackages is true', async () => {
  const projectAManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      'b': 'workspace:*',
    },
  }
  const projectBManifest = {
    name: 'b',
    version: '1.0.0',
  }

  preparePackages([
    {
      location: 'a',
      package: projectAManifest,
    },
    {
      location: 'b',
      package: projectBManifest,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: projectAManifest,
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: projectBManifest,
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ]

  // Initial install with injectWorkspacePackages: true
  await mutateModules(importers, testDefaults({
    allProjects,
    injectWorkspacePackages: true,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()

  // When injectWorkspacePackages is true AND dedupeInjectedDeps is enabled (default),
  // workspace packages should use link: protocol when deduplication is possible
  expect(lockfile.importers.a.dependencies!.b.version).toBe('link:../b')
  const initialProtocol = lockfile.importers.a.dependencies!.b.version

  // Add a regular dependency to package a manifest
  projectAManifest.dependencies['is-positive'] = '1.0.0'

  // Run install again with the new dependency
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects,
    injectWorkspacePackages: true,
  }))

  const lockfileAfterAdd = rootModules.readLockfile()

  // Verify workspace package still uses the same protocol
  expect(lockfileAfterAdd.importers.a.dependencies!.b.version).toBe(initialProtocol)

  // Remove the regular dependency from manifest
  delete projectAManifest.dependencies['is-positive']

  // Run install again
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects,
    injectWorkspacePackages: true,
  }))

  const lockfileAfterRemove = rootModules.readLockfile()

  // Verify workspace package STILL uses the same protocol (the bug was it would change)
  expect(lockfileAfterRemove.importers.a.dependencies!.b.version).toBe(initialProtocol)
})

test('workspace packages should maintain consistent protocol after pnpm rm when injectWorkspacePackages is true', async () => {
  const projectAManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      'b': 'workspace:*',
      'is-positive': '1.0.0',
    },
  }
  const projectBManifest = {
    name: 'b',
    version: '1.0.0',
  }

  preparePackages([
    {
      location: 'a',
      package: projectAManifest,
    },
    {
      location: 'b',
      package: projectBManifest,
    },
  ])

  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: projectAManifest,
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: projectBManifest,
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ]

  // Initial full install
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects,
    injectWorkspacePackages: true,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  expect(lockfile.importers.a.dependencies!.b.version).toBe('link:../b')
  const initialProtocol = lockfile.importers.a.dependencies!.b.version

  // Remove a dependency using uninstallSome (simulates `pnpm rm is-positive` in package a)
  delete projectAManifest.dependencies['is-positive']
  await mutateModules([
    {
      mutation: 'uninstallSome',
      dependencyNames: ['is-positive'],
      rootDir: path.resolve('a') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects: allProjects.map((p) =>
      p.rootDir === (path.resolve('a') as ProjectRootDir)
        ? { ...p, manifest: projectAManifest }
        : p
    ),
    injectWorkspacePackages: true,
  }))

  const lockfileAfterRm = rootModules.readLockfile()

  // Verify workspace package still uses link: protocol after pnpm rm
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe(initialProtocol)
})
