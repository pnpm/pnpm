import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { mutateModules, MutatedProject } from '@pnpm/core'
import { testDefaults } from '../utils'

test('dedupe direct dependencies', async () => {
  const projects = preparePackages([
    {
      location: '',
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
      rootDir: process.cwd(),
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
          'is-positive': '1.0.0',
          'is-odd': '1.0.0',
        },
      },
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, dedupeDirectDeps: true }))
  await projects['project-2'].has('is-negative')

  allProjects[0].manifest.dependencies['is-negative'] = '1.0.0'
  allProjects[1].manifest.dependencies['is-positive'] = '1.0.0'
  allProjects[1].manifest.dependencies['is-odd'] = '2.0.0'
  await mutateModules(importers, await testDefaults({ allProjects, dedupeDirectDeps: true }))

  expect(Array.from(fs.readdirSync('node_modules').sort())).toEqual([
    '.modules.yaml',
    '.pnpm',
    'is-negative',
    'is-odd',
    'is-positive',
  ])
  expect(fs.readdirSync('project-2/node_modules').sort()).toEqual(['is-odd'])
})
