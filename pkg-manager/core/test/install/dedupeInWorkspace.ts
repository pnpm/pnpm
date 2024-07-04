import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { mutateModules, type MutatedProject } from '@pnpm/core'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectRootDir } from '@pnpm/types'
import { testDefaults } from '../utils'

test('pick common range for a dependency used in two workspace projects when resolution mode is highest', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects, lockfileOnly: true, resolutionMode: 'highest' }))

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('pick common range for a dependency used in two workspace projects when resolution mode is lowest-direct', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.1.0',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects, lockfileOnly: true, resolutionMode: 'lowest-direct' }))

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
})
