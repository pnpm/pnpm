import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { mutateModules, MutatedProject } from '@pnpm/core'
import { addDistTag } from '@pnpm/registry-mock'
import { testDefaults } from '../utils'

test('pick common range for a dependency used in two workspace projects', async () => {
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
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
      rootDir: path.resolve('project-1'),
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
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, lockfileOnly: true }))

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
})
