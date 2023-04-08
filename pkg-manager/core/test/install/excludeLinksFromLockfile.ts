import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  mutateModules,
  type MutatedProject,
  type ProjectOptions,
} from '@pnpm/core'
import { type LockfileV6 } from '@pnpm/lockfile-types'
import { preparePackages, tempDir } from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import readYamlFile from 'read-yaml-file'
import { testDefaults } from '../utils'

test('links are not added to the lockfile when excludeLinksFromLockfile is true', async () => {
  const externalPkg1 = tempDir(false)
  const externalPkg2 = tempDir(false)
  const externalPkg3 = tempDir(false)
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
  const project1Dir = path.resolve('project-1')
  const project2Dir = path.resolve('project-2')
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'external-1': `link:${path.relative(project1Dir, externalPkg1)}`,
        },
      },
      rootDir: project1Dir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
          'external-2': `link:${path.relative(project2Dir, externalPkg2)}`,
        },
      },
      rootDir: project2Dir,
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, excludeLinksFromLockfile: true }))
  const lockfile: LockfileV6 = await readYamlFile(WANTED_LOCKFILE)
  expect(lockfile.importers['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers['project-2'].dependencies?.['external-2']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-2'))).toBeTruthy()

  await rimraf('node_modules')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')

  await mutateModules(importers, await testDefaults({ allProjects, excludeLinksFromLockfile: true, frozenLockfile: true }))
  expect(lockfile.importers['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers['project-2'].dependencies?.['external-2']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-2'))).toBeTruthy()

  await rimraf('node_modules')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')

  await mutateModules(importers, await testDefaults({ allProjects, excludeLinksFromLockfile: true, frozenLockfile: false, preferFrozenLockfile: false }))
  expect(lockfile.importers['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers['project-2'].dependencies?.['external-2']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-2'))).toBeTruthy()

  delete allProjects[1].manifest.dependencies!['external-2']
  allProjects[1].manifest.dependencies!['external-3'] = `link:${path.relative(project2Dir, externalPkg3)}`
  await mutateModules(importers, await testDefaults({ allProjects, excludeLinksFromLockfile: true }))
  expect(lockfile.importers['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers['project-2'].dependencies?.['external-2']).toBeUndefined()
  expect(lockfile.importers['project-2'].dependencies?.['external-3']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1'))).toBeTruthy()
  // expect(fs.existsSync(path.resolve('project-2/node_modules/external-2'))).toBeFalsy() // Should we remove external links that are not in deps anymore?
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-3'))).toBeTruthy()
})
