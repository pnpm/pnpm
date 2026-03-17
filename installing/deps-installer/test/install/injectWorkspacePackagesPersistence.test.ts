import path from 'node:path'

import { assertProject } from '@pnpm/assert-project'
import { mutateModules, mutateModulesInSingleProject, type ProjectOptions } from '@pnpm/core'
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
