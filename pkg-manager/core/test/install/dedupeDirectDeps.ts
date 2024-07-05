import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { mutateModules, type MutatedProject } from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
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
    {
      location: 'project-3',
      package: { name: 'project-3' },
    },
  ])
  fs.mkdirSync('node_modules/foo', { recursive: true })
  fs.writeFileSync('node_modules/foo/package.json', JSON.stringify({ name: 'foo', version: '1.0.0' }))

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
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
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/hello-world-js-bin': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-3',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/hello-world-js-bin': '1.0.0',
        },
      },
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true }))
  projects['project-2'].has('@pnpm.e2e/hello-world-js-bin')
  projects['project-3'].has('@pnpm.e2e/hello-world-js-bin')

  allProjects[0].manifest.dependencies['@pnpm.e2e/hello-world-js-bin'] = '1.0.0'
  allProjects[1].manifest.dependencies['is-positive'] = '1.0.0'
  allProjects[1].manifest.dependencies['is-odd'] = '2.0.0'
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true }))

  expect(Array.from(fs.readdirSync('node_modules').sort())).toEqual([
    '.bin',
    '.modules.yaml',
    '.pnpm',
    '@pnpm.e2e',
    'foo',
    'is-odd',
    'is-positive',
  ])
  expect(Array.from(fs.readdirSync('node_modules/@pnpm.e2e'))).toEqual(['hello-world-js-bin'])
  expect(fs.readdirSync('project-2/node_modules').sort()).toEqual(['is-odd'])
  projects['project-3'].hasNot('@pnpm.e2e/hello-world-js-bin')
  expect(fs.existsSync('project-3/node_modules')).toBeFalsy()

  // Test the same with headless install
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, frozenLockfile: true }))
  expect(fs.readdirSync('project-2/node_modules').sort()).toEqual(['is-odd'])
  projects['project-3'].hasNot('@pnpm.e2e/hello-world-js-bin')
  expect(fs.existsSync('project-3/node_modules')).toBeFalsy()
})

test('dedupe direct dependencies after public hoisting', async () => {
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
      rootDir: process.cwd() as ProjectRootDir,
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
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const opts = testDefaults({
    allProjects,
    dedupeDirectDeps: true,
    publicHoistPattern: ['@pnpm.e2e/dep-of-pkg-with-1-dep'],
  })
  await mutateModules(importers, opts)
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-2'].hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  expect(Array.from(fs.readdirSync('node_modules/@pnpm.e2e').sort())).toEqual([
    'dep-of-pkg-with-1-dep',
    'pkg-with-1-dep',
  ])
  expect(fs.existsSync('project-2/node_modules')).toBeFalsy()

  // Test the same with headless install
  rimraf('node_modules')
  await mutateModules(importers, { ...opts, frozenLockfile: true })
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-2'].hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  expect(Array.from(fs.readdirSync('node_modules/@pnpm.e2e').sort())).toEqual([
    'dep-of-pkg-with-1-dep',
    'pkg-with-1-dep',
  ])
  expect(fs.existsSync('project-2/node_modules')).toBeFalsy()
})
