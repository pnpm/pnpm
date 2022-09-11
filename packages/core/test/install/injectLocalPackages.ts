import fs from 'fs'
import path from 'path'
import assertProject from '@pnpm/assert-project'
import { MutatedProject, mutateModules } from '@pnpm/core'
import { preparePackages } from '@pnpm/prepare'
import rimraf from '@zkochan/rimraf'
import pathExists from 'path-exists'
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
    'project-3': {
      '1.0.0': {
        dir: path.resolve('project-3'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  await projects['project-3'].has('is-positive')
  await projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    expect(lockfile.packages['file:project-2_is-positive@2.0.0']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
      id: 'file:project-2',
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'project-1': 'file:project-1_is-positive@2.0.0',
      },
      transitivePeerDependencies: ['is-positive'],
      dev: false,
    })

    const modulesState = await rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }

  await rimraf('node_modules')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')
  await rimraf('project-3/node_modules')

  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  await projects['project-3'].has('is-positive')
  await projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  await mutateModules(importers, await testDefaults({ allProjects, workspacePackages }))
  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        'is-negative': '2.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    const modulesState = await rootModules.readModulesManifest()
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
    'project-3': {
      '1.0.0': {
        dir: path.resolve('project-3'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  await projects['project-3'].has('is-positive')
  await projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    expect(lockfile.packages['file:project-2_is-positive@2.0.0']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
      id: 'file:project-2',
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'project-1': 'file:project-1_is-positive@2.0.0',
      },
      transitivePeerDependencies: ['is-positive'],
      dev: false,
    })

    const modulesState = await rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }

  await rimraf('node_modules')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')
  await rimraf('project-3/node_modules')

  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  await projects['project-3'].has('is-positive')
  await projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  writeJsonFile('project-1/package.json', allProjects[0].manifest)
  await mutateModules(importers, await testDefaults({ allProjects, workspacePackages }))
  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        'is-negative': '2.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    const modulesState = await rootModules.readModulesManifest()
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
    'project-3': {
      '1.0.0': {
        dir: path.resolve('project-3'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  await projects['project-3'].has('is-positive')
  await projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  const rootModules = assertProject(process.cwd())
  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    expect(lockfile.packages['file:project-2_is-positive@2.0.0']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
      id: 'file:project-2',
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'project-1': 'file:project-1_is-positive@2.0.0',
      },
      transitivePeerDependencies: ['is-positive'],
      dev: false,
    })

    const modulesState = await rootModules.readModulesManifest()
    expect(modulesState?.injectedDeps?.['project-1'].length).toEqual(2)
    expect(modulesState?.injectedDeps?.['project-1'][0]).toContain(`node_modules${path.sep}.pnpm`)
    expect(modulesState?.injectedDeps?.['project-1'][1]).toContain(`node_modules${path.sep}.pnpm`)
  }

  await rimraf('node_modules')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')
  await rimraf('project-3/node_modules')

  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  await projects['project-3'].has('is-positive')
  await projects['project-3'].has('project-2')

  expect(fs.readdirSync('node_modules/.pnpm').length).toBe(8)

  // The injected project is updated when one of its dependencies needs to be updated
  allProjects[0].manifest.dependencies!['is-negative'] = '2.0.0'
  writeJsonFile('project-1/package.json', allProjects[0].manifest)
  await mutateModules(importers, await testDefaults({
    allProjects,
    workspacePackages,
  }))
  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        'is-negative': '2.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    const modulesState = await rootModules.readModulesManifest()
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
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  expect(await pathExists(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()

  const rootModules = assertProject(process.cwd())
  const lockfile = await rootModules.readLockfile()
  expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
    'project-1': {
      injected: true,
    },
  })
  expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
    resolution: {
      directory: 'project-1',
      type: 'directory',
    },
    id: 'file:project-1',
    name: 'project-1',
    version: '1.0.0',
    peerDependencies: {
      'is-positive': '1.0.0',
    },
    dependencies: {
      'is-negative': '1.0.0',
      'is-positive': '1.0.0',
    },
    dev: false,
  })

  await rimraf('node_modules')
  await rimraf('project-1/main.js')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')

  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
    workspacePackages,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  expect(await pathExists(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()
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
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  expect(await pathExists(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()

  const rootModules = assertProject(process.cwd())
  const lockfile = await rootModules.readLockfile()
  expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
    resolution: {
      directory: 'project-1',
      type: 'directory',
    },
    id: 'file:project-1',
    name: 'project-1',
    version: '1.0.0',
    peerDependencies: {
      'is-positive': '1.0.0',
    },
    dependencies: {
      'is-negative': '1.0.0',
      'is-positive': '1.0.0',
    },
    dev: false,
  })

  await rimraf('node_modules')
  await rimraf('project-1/main.js')
  await rimraf('project-1/node_modules')
  await rimraf('project-2/node_modules')

  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
  }))

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await projects['project-1'].hasNot('is-positive')

  await projects['project-2'].has('is-positive')
  await projects['project-2'].has('project-1')

  expect(await pathExists(path.resolve('project-2/node_modules/project-1/main.js'))).toBeTruthy()
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
    'project-3': {
      '1.0.0': {
        dir: path.resolve('project-3'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    nodeLinker: 'hoisted',
    workspacePackages,
  }))

  const rootModules = assertProject(process.cwd())
  await rootModules.has('is-negative')
  await rootModules.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await rootModules.has('is-positive')

  await projects['project-2'].has('project-1')
  await projects['project-2'].has('project-1/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')

  await projects['project-3'].has('project-1')
  await projects['project-3'].has('project-2')
  await projects['project-3'].has('is-positive')

  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    expect(lockfile.packages['file:project-2_is-positive@2.0.0']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
      id: 'file:project-2',
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        'project-1': 'file:project-1_is-positive@2.0.0',
      },
      transitivePeerDependencies: ['is-positive'],
      dev: false,
    })

    const modulesState = await rootModules.readModulesManifest()
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
    'project-3': {
      '1.0.0': {
        dir: path.resolve('project-3'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    nodeLinker: 'hoisted',
    workspacePackages,
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
  await rootModules.has('is-negative')
  await rootModules.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await rootModules.has('is-positive')

  await projects['project-2'].has('project-1')
  await projects['project-2'].has('project-1/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')

  await projects['project-3'].has('project-1')
  await projects['project-3'].has('project-2')
  await projects['project-3'].has('is-positive')

  {
    const lockfile = await rootModules.readLockfile()
    expect(lockfile.importers['project-2'].dependenciesMeta).toEqual({
      'project-1': {
        injected: true,
      },
    })
    expect(lockfile.packages['file:project-1_is-positive@1.0.0']).toEqual({
      resolution: {
        directory: 'project-1',
        type: 'directory',
      },
      id: 'file:project-1',
      name: 'project-1',
      version: '1.0.0',
      peerDependencies: {
        'is-positive': '>=1.0.0',
      },
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
      dev: false,
    })
    expect(lockfile.packages['file:project-2_is-positive@2.0.0']).toEqual({
      resolution: {
        directory: 'project-2',
        type: 'directory',
      },
      id: 'file:project-2',
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        'project-1': 'file:project-1_is-positive@2.0.0',
      },
      transitivePeerDependencies: ['is-positive'],
      dev: false,
    })

    const modulesState = await rootModules.readModulesManifest()
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: project3Manifest,
      rootDir: path.resolve('project-3'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
    'project-3': {
      '1.0.0': {
        dir: path.resolve('project-3'),
        manifest: project2Manifest,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    nodeLinker: 'hoisted',
    workspacePackages,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = await rootModules.readLockfile()
  expect(lockfile.packages?.['file:project-2_project-1@project-1'].dependencies?.['project-1']).toEqual('link:project-1')
})

// There was a bug related to this. The manifests in the workspacePackages object were modified
test('do not modify the manifest of the injected workpspace project', async () => {
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
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: project1Manifest,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: project2Manifest,
      },
    },
  }
  const [project1] = await mutateModules(importers, await testDefaults({
    allProjects,
    workspacePackages,
  }))
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
