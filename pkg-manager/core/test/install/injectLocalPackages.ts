import fs from 'fs'
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { type MutatedProject, mutateModules, type ProjectOptions } from '@pnpm/core'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as writeJsonFile } from 'write-json-file'
import { testDefaults } from '../utils'

test('inject local packages', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-2': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '2.0.0',
    },
    dependenciesMeta: {
      'project-2': {
        injected: true,
      },
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-2@file:project-2']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
    })
    expect(lockfile.snapshots['project-2@file:project-2(is-positive@2.0.0)']).toEqual({
      dependencies: {
        'project-1': 'file:project-1(is-positive@2.0.0)',
      },
      transitivePeerDependencies: ['is-positive'],
    })

    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }

  rimraf('node_modules')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')
  rimraf('project-3/node_modules')

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    frozenLockfile: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  await mutateModules(importers, testDefaults({ autoInstallPeers: false, allProjects }))
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        'is-negative': '2.0.0',
        'is-positive': '1.0.0',
      },
    })
    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }
})

test('inject local packages declared via file protocol', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'file:../project-1',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-2': 'file:../project-2',
    },
    devDependencies: {
      'is-positive': '2.0.0',
    },
    dependenciesMeta: {
      'project-2': {
        injected: true,
      },
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    expect(lockfile.snapshots['project-2@file:project-2(is-positive@2.0.0)']).toEqual({
      dependencies: {
        'project-1': 'file:project-1(is-positive@2.0.0)',
      },
      transitivePeerDependencies: ['is-positive'],
    })
    expect(lockfile.packages['project-2@file:project-2']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
    })

    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }

  rimraf('node_modules')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')
  rimraf('project-3/node_modules')

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    frozenLockfile: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  writeJsonFile('project-1/package.json', allProjects[0].manifest)
  await mutateModules(importers, testDefaults({ autoInstallPeers: false, allProjects }))
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        'is-negative': '2.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }
})

test('inject local packages when the file protocol is used', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'file:../project-1',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-2': 'file:../project-2',
    },
    devDependencies: {
      'is-positive': '2.0.0',
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    expect(lockfile.snapshots['project-2@file:project-2(is-positive@2.0.0)']).toEqual({
      dependencies: {
        'project-1': 'file:project-1(is-positive@2.0.0)',
      },
      transitivePeerDependencies: ['is-positive'],
    })
    expect(lockfile.packages['project-2@file:project-2']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
    })

    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }

  rimraf('node_modules')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')
  rimraf('project-3/node_modules')

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    frozenLockfile: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  writeJsonFile('project-1/package.json', allProjects[0].manifest)
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
  }))
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        'is-negative': '2.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }
})

test('inject local packages and relink them after build', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '1.0.0',
    },
    scripts: {
      prepublishOnly: 'touch main.js',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
      'project-1': 'workspace:1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
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
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  expect(fs.existsSync(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
    'project-1': {
      injected: true,
    },
  })
  expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
    dependencies: {
      'is-negative': '1.0.0',
      'is-positive': '1.0.0',
    },
  })
  expect(lockfile.packages['project-1@file:project-1']).toEqual({
    resolution: {
      directory: 'project-1',
      type: 'directory',
    },
    peerDependencies: {
      'is-positive': '1.0.0',
    },
  })

  rimraf('node_modules')
  rimraf('project-1/main.js')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    frozenLockfile: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  expect(fs.existsSync(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()
})

test('inject local packages and relink them after build (file protocol is used)', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '1.0.0',
    },
    scripts: {
      prepublishOnly: 'touch main.js',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
      'project-1': 'file:../project-1',
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
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
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ autoInstallPeers: false, allProjects }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  expect(fs.existsSync(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
    dependencies: {
      'is-negative': '1.0.0',
      'is-positive': '1.0.0',
    },
  })
  expect(lockfile.packages['project-1@file:project-1']).toEqual({
    resolution: {
      directory: 'project-1',
      type: 'directory',
    },
    peerDependencies: {
      'is-positive': '1.0.0',
    },
  })

  rimraf('node_modules')
  rimraf('project-1/main.js')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    frozenLockfile: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].hasNot('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  expect(fs.existsSync(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()
})

test('inject local packages when node-linker is hoisted', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-2': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '2.0.0',
    },
    dependenciesMeta: {
      'project-2': {
        injected: true,
      },
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    nodeLinker: 'hoisted',
  }))

  const rootModules = assertProject(process.cwd())
  rootModules.has('is-negative')
  rootModules.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  rootModules.has('is-positive')

  projects['project-2'].has('project-1')
  projects['project-2'].has('project-1/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')

  projects['project-3'].has('project-1')
  projects['project-3'].has('project-2')
  projects['project-3'].has('is-positive')

  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    expect(lockfile.snapshots['project-2@file:project-2(is-positive@2.0.0)']).toEqual({
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        'project-1': 'file:project-1(is-positive@2.0.0)',
      },
      transitivePeerDependencies: ['is-positive'],
    })
    expect(lockfile.packages['project-2@file:project-2']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
    })

    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toEqual(path.join('project-2', 'node_modules', 'project-1'))
    expect(modulesState?.injectedDeps?.['project-1'][1]).toEqual(path.join('project-3', 'node_modules', 'project-1'))
  }
})

test('inject local packages when node-linker is hoisted and dependenciesMeta is set via a hook', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-2': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '2.0.0',
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    nodeLinker: 'hoisted',
    hooks: {
      readPackage: (manifest: any) => { // eslint-disable-line
        if (manifest.name === 'project-2') {
          manifest.dependenciesMeta = {
            'project-1': {
              injected: true,
            },
          }
        }
        if (manifest.name === 'project-3') {
          manifest.dependenciesMeta = {
            'project-2': {
              injected: true,
            },
          }
        }
        return manifest
      },
    },
  } as any)) // eslint-disable-line

  const rootModules = assertProject(process.cwd())
  rootModules.has('is-negative')
  rootModules.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  rootModules.has('is-positive')

  projects['project-2'].has('project-1')
  projects['project-2'].has('project-1/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')

  projects['project-3'].has('project-1')
  projects['project-3'].has('project-2')
  projects['project-3'].has('is-positive')

  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.snapshots['project-1@file:project-1(is-positive@1.0.0)']).toEqual({
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    })
    expect(lockfile.packages['project-1@file:project-1']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
    })
    expect(lockfile.snapshots['project-2@file:project-2(is-positive@2.0.0)']).toEqual({
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        'project-1': 'file:project-1(is-positive@2.0.0)',
      },
      transitivePeerDependencies: ['is-positive'],
    })
    expect(lockfile.packages['project-2@file:project-2']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
    })

    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toEqual(path.join('project-2', 'node_modules', 'project-1'))
    expect(modulesState?.injectedDeps?.['project-1'][1]).toEqual(path.join('project-3', 'node_modules', 'project-1'))
  }
})

test('peer dependency of injected project should be resolved correctly', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {},
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    devDependencies: {
      'project-1': 'workspace:1.0.0',
    },
    peerDependencies: {
      'project-1': 'workspace:^1.0.0',
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
      'project-2': 'workspace:1.0.0',
    },
    dependenciesMeta: {
      'project-2': {
        injected: true,
      },
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    nodeLinker: 'hoisted',
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  expect(lockfile.snapshots?.['project-2@file:project-2(project-1@project-1)'].dependencies?.['project-1']).toEqual('link:project-1')
})

// There was a bug related to this. The manifests in the workspacePackages object were modified
test('do not modify the manifest of the injected workspace project', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
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
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const [project1] = (await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
  }))).updatedProjects
  expect(project1.manifest).toStrictEqual({
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  })
})

test('injected package is kept up-to-date when it is hoisted to multiple places', async () => {
  // We create a root project with is-positive in the dependencies, so that the local is-positive
  // inside project-1 and project-2 will be nested into their node_modules
  const rootProjectManifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-positive': '2.0.0',
    },
  }
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-positive': 'workspace:1.0.0',
    },
    dependenciesMeta: {
      'is-positive': {
        injected: true,
      },
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'is-positive': 'workspace:1.0.0',
    },
    dependenciesMeta: {
      'is-positive': {
        injected: true,
      },
    },
  }
  const project3Manifest = {
    name: 'is-positive',
    version: '1.0.0',
    scripts: {
      prepare: 'node -e "require(\'fs\').writeFileSync(\'prepare.txt\', \'prepare\', \'utf8\')"',
    },
  }
  const projects = preparePackages([
    {
      location: '',
      package: rootProjectManifest,
    },
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
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
      manifest: rootProjectManifest,
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    allProjects,
    dedupeInjectedDeps: false,
    nodeLinker: 'hoisted',
  }))

  projects['project-1'].has('is-positive/prepare.txt')
  projects['project-2'].has('is-positive/prepare.txt')

  const rootModules = assertProject(process.cwd())
  const modulesState = rootModules.readModulesManifest()
  expect(modulesState?.injectedDeps?.['project-3'].length).toEqual(2)
  expect(modulesState?.injectedDeps?.['project-3'][0]).toEqual(path.join('project-1', 'node_modules', 'is-positive'))
  expect(modulesState?.injectedDeps?.['project-3'][1]).toEqual(path.join('project-2', 'node_modules', 'is-positive'))
})

test('relink injected dependency on install by default', async () => {
  const depManifest = {
    name: 'dep',
    version: '1.0.0',
  }
  const mainManifest = {
    name: 'main',
    version: '1.0.0',
    dependencies: {
      dep: 'workspace:1.0.0',
    },
    dependenciesMeta: {
      dep: {
        injected: true,
      },
    },
  }
  preparePackages([
    {
      location: 'dep',
      package: depManifest,
    },
    {
      location: 'main',
      package: mainManifest,
    },
  ])
  fs.writeFileSync('dep/index.js', 'console.log("dep")')
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('dep') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('main') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: depManifest,
      rootDir: path.resolve('dep') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: mainManifest,
      rootDir: path.resolve('main') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    allProjects,
    dedupeInjectedDeps: false,
    packageImportMethod: 'hardlink',
    fastUnpack: false,
  }))

  const indexJsPath = path.resolve('main/node_modules/dep/index.js')
  const getInode = () => fs.statSync(indexJsPath).ino
  const storeInode = getInode()

  // rewriting index.js, to destroy the link
  fs.unlinkSync(indexJsPath)
  fs.writeFileSync(indexJsPath, 'console.log("dep updated")')

  expect(storeInode).not.toEqual(getInode())

  await mutateModules(importers, testDefaults({
    allProjects,
    dedupeInjectedDeps: false,
    packageImportMethod: 'hardlink',
    fastUnpack: false,
  }))

  expect(storeInode).toEqual(getInode())
})

test('do not relink injected dependency on install when disableRelinkLocalDirDeps is set to true', async () => {
  const depManifest = {
    name: 'dep',
    version: '1.0.0',
  }
  const mainManifest = {
    name: 'main',
    version: '1.0.0',
    dependencies: {
      dep: 'workspace:1.0.0',
    },
    dependenciesMeta: {
      dep: {
        injected: true,
      },
    },
  }
  preparePackages([
    {
      location: 'dep',
      package: depManifest,
    },
    {
      location: 'main',
      package: mainManifest,
    },
  ])
  fs.writeFileSync('dep/index.js', 'console.log("dep")')
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('dep') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('main') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: depManifest,
      rootDir: path.resolve('dep') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: mainManifest,
      rootDir: path.resolve('main') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    allProjects,
    dedupeInjectedDeps: false,
    packageImportMethod: 'hardlink',
    fastUnpack: false,
  }))

  const pkgJsonPath = path.resolve('main/node_modules/dep/package.json')
  const getInode = () => fs.statSync(pkgJsonPath).ino
  const storeInode = getInode()

  // rewriting index.js, to destroy the link
  const pkgJsonContent = fs.readFileSync(pkgJsonPath, 'utf8')
  fs.unlinkSync(pkgJsonPath)
  fs.writeFileSync(pkgJsonPath, pkgJsonContent)

  const newInode = getInode()

  expect(storeInode).not.toEqual(newInode)

  await mutateModules(importers, testDefaults({
    allProjects,
    dedupeInjectedDeps: false,
    packageImportMethod: 'hardlink',
    fastUnpack: false,
    disableRelinkLocalDirDeps: true,
  }))

  expect(newInode).toEqual(getInode())
})

test('injected local packages are deduped', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  const project3Manifest = {
    name: 'project-3',
    version: '1.0.0',
    dependencies: {
      'project-2': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '2.0.0',
    },
    dependenciesMeta: {
      'project-2': {
        injected: true,
      },
    },
  }
  const project4Manifest = {
    name: 'project-4',
    version: '1.0.0',
    dependencies: {
      'project-2': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-2': {
        injected: true,
      },
    },
  }
  const project5Manifest = {
    name: 'project-5',
    version: '1.0.0',
    dependencies: {
      'project-4': 'workspace:1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'project-4': {
        injected: true,
      },
    },
  }
  const projects = preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
    {
      location: 'project-3',
      package: project3Manifest,
    },
    {
      location: 'project-4',
      package: project4Manifest,
    },
    {
      location: 'project-5',
      package: project5Manifest,
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
    {
      mutation: 'install',
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-4') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-5') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project4Manifest,
      rootDir: path.resolve('project-4') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project5Manifest,
      rootDir: path.resolve('project-5') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    autoInstallPeers: true,
    allProjects,
    dedupeInjectedDeps: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].has('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(7)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['project-1@file:project-1(is-positive@1.0.0)']).toBeFalsy()
    expect(lockfile.snapshots['project-2@file:project-2(is-positive@2.0.0)']).toEqual({
      dependencies: {
        'project-1': 'file:project-1(is-positive@2.0.0)',
      },
      transitivePeerDependencies: ['is-positive'],
    })
    expect(lockfile.packages['project-2@file:project-2']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
    })

    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(1)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
  }

  rimraf('node_modules')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')
  rimraf('project-3/node_modules')

  await mutateModules(importers, testDefaults({
    autoInstallPeers: true,
    allProjects,
    dedupeInjectedDeps: true,
    frozenLockfile: true,
  }))

  projects['project-1'].has('is-negative')
  projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['project-1'].has('is-positive')

  projects['project-2'].has('is-positive')
  projects['project-2'].has('project-1')

  projects['project-3'].has('is-positive')
  projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(7)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  await mutateModules(importers, testDefaults({ autoInstallPeers: true, allProjects, dedupeInjectedDeps: true }))
  {
    const lockfile = rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['project-1@file:project-1(is-positive@1.0.0)']).toBeFalsy()
    const modulesState = rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(1)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
  }
})
