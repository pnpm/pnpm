import { prepareEmpty } from '@pnpm/prepare'
import {
  mutateModules,
  type MutatedProject,
} from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import { testDefaults } from '../utils'

test('spec not specified in package.json.dependencies', async () => {
  prepareEmpty()

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      rootDir: process.cwd(),
    },
  ]
  {
    const { stats } = await mutateModules(importers, await testDefaults({ allProjects }))
    expect(stats.added).toEqual(1)
    expect(stats.removed).toEqual(0)
    expect(stats.linkedToRoot).toEqual(1)
  }
  rimraf('node_modules')
  {
    const { stats } = await mutateModules(importers, await testDefaults({ allProjects, frozenLockfile: true }))
    expect(stats.added).toEqual(1)
    expect(stats.removed).toEqual(0)
    expect(stats.linkedToRoot).toEqual(1)
  }
  {
    const { stats } = await mutateModules([
      {
        mutation: 'uninstallSome',
        dependencyNames: ['is-positive'],
        rootDir: process.cwd(),
      },
    ], await testDefaults({ allProjects, frozenLockfile: true }))
    expect(stats.added).toEqual(0)
    expect(stats.removed).toEqual(1)
    expect(stats.linkedToRoot).toEqual(0)
  }
})
