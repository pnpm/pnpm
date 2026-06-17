import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { mutateModules, mutateModulesInSingleProject, type ProjectOptions } from '@pnpm/installing.deps-installer'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

test('workspace packages should maintain link: protocol after single-project pnpm rm with injectWorkspacePackages', async () => {
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

  // Initial full install with all projects
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

  // Remove a dependency using mutateModulesInSingleProject.
  // This is the code path used by `pnpm rm` when run from within a single
  // workspace package directory. It passes allProjects with only the single
  // project, so ctx.projects won't contain the other workspace packages.
  // The workspacePackages map must still include all workspace packages
  // for resolution to work.
  delete projectAManifest.dependencies['is-positive']
  const workspacePackages = new Map([
    ['a', new Map([
      ['1.0.0', {
        rootDir: path.resolve('a') as ProjectRootDir,
        manifest: projectAManifest,
      }],
    ])],
    ['b', new Map([
      ['1.0.0', {
        rootDir: path.resolve('b') as ProjectRootDir,
        manifest: projectBManifest,
      }],
    ])],
  ])
  await mutateModulesInSingleProject(
    {
      binsDir: path.resolve('a', 'node_modules', '.bin'),
      dependencyNames: ['is-positive'],
      manifest: projectAManifest,
      mutation: 'uninstallSome',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    testDefaults({
      workspacePackages,
      injectWorkspacePackages: true,
    })
  )

  const lockfileAfterRm = rootModules.readLockfile()

  // Without the fix, workspace dep 'b' would switch from link: to file: protocol
  // because dedupeInjectedDeps couldn't identify 'b' as a workspace package
  // when only package 'a' was in the projects list.
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe('link:../b')
})

test('workspace packages with their own dependencies should maintain link: protocol after single-project pnpm rm with injectWorkspacePackages', async () => {
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
    dependencies: {
      'is-negative': '1.0.0',
    },
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

  // Initial full install with all projects
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

  // Same single-project rm path as the test above, but `b` has its own dependency
  // (`is-negative`). The injected file: dep then has children, which hits a separate
  // branch in dedupeInjectedDeps.
  delete projectAManifest.dependencies['is-positive']
  const workspacePackages = new Map([
    ['a', new Map([
      ['1.0.0', {
        rootDir: path.resolve('a') as ProjectRootDir,
        manifest: projectAManifest,
      }],
    ])],
    ['b', new Map([
      ['1.0.0', {
        rootDir: path.resolve('b') as ProjectRootDir,
        manifest: projectBManifest,
      }],
    ])],
  ])
  await mutateModulesInSingleProject(
    {
      binsDir: path.resolve('a', 'node_modules', '.bin'),
      dependencyNames: ['is-positive'],
      manifest: projectAManifest,
      mutation: 'uninstallSome',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    testDefaults({
      workspacePackages,
      injectWorkspacePackages: true,
    })
  )

  const lockfileAfterRm = rootModules.readLockfile()

  // Without the fix, dedupeInjectedDeps would skip dedupe when the injected dep had
  // children and the target workspace project wasn't in the current resolution, so
  // workspace dep 'b' would switch from link: to file:.
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe('link:../b')
})

test('peer-resolved workspace packages should keep their file: protocol after single-project pnpm rm with injectWorkspacePackages', async () => {
  const projectAManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      'b': 'workspace:*',
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }
  const projectBManifest: { name: string, version: string, dependencies: Record<string, string>, peerDependencies: Record<string, string> } = {
    name: 'b',
    version: '1.0.0',
    dependencies: {},
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
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

  // Initial full install with all projects
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
    autoInstallPeers: false,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  // With a peer dep on `b`, the injected resolution depends on `a`'s peer context, so the
  // entry stays in file: form rather than collapsing to link:../b.
  const initialVersion = lockfile.importers.a.dependencies!.b.version
  expect(initialVersion).not.toBe('link:../b')
  expect(initialVersion.startsWith('file:')).toBe(true)

  // Single-project rm of an unrelated dep should preserve the peer-resolved file: form.
  delete projectAManifest.dependencies['is-negative']
  const workspacePackages = new Map([
    ['a', new Map([
      ['1.0.0', {
        rootDir: path.resolve('a') as ProjectRootDir,
        manifest: projectAManifest,
      }],
    ])],
    ['b', new Map([
      ['1.0.0', {
        rootDir: path.resolve('b') as ProjectRootDir,
        manifest: projectBManifest,
      }],
    ])],
  ])
  await mutateModulesInSingleProject(
    {
      binsDir: path.resolve('a', 'node_modules', '.bin'),
      dependencyNames: ['is-negative'],
      manifest: projectAManifest,
      mutation: 'uninstallSome',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    testDefaults({
      workspacePackages,
      injectWorkspacePackages: true,
      autoInstallPeers: false,
    })
  )

  const lockfileAfterRm = rootModules.readLockfile()

  // The fast-path must skip dedupe for peer-suffixed depPaths. Without the peer-suffix
  // check, dedupeInjectedDeps would collapse the peer-resolved file: entry to link:../b
  // and lose the importer's peer context.
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe(initialVersion)
})
