import path from 'path'
import assertProject from '@pnpm/assert-project'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { readCurrentLockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { ProjectManifest } from '@pnpm/types'
import {
  addDependenciesToPackage,
  MutatedProject,
  mutateModules,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import { createPeersFolderSuffix } from 'dependency-path'
import loadJsonFile from 'load-json-file'
import exists from 'path-exists'
import pick from 'ramda/src/pick'
import sinon from 'sinon'
import writeYamlFile from 'write-yaml-file'
import { testDefaults } from '../utils'

test('install only the dependencies of the specified importer', async () => {
  const projects = preparePackages([
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
          'is-positive': '1.0.0',
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
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, lockfileOnly: true }))

  await mutateModules(importers.slice(0, 1), await testDefaults({ allProjects }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')

  const rootModules = assertProject(process.cwd())
  await rootModules.has('.pnpm/is-positive@1.0.0')
  await rootModules.hasNot('.pnpm/is-negative@1.0.0')
})

test('install only the dependencies of the specified importer. The current lockfile has importers that do not exist anymore', async () => {
  preparePackages([
    {
      location: 'project-1',
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
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
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
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-3',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/foobar': '100.0.0',
        },
      },
      rootDir: path.resolve('project-3'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, hoistPattern: '*' }))
  await mutateModules(importers.slice(0, 2), await testDefaults({ allProjects, lockfileOnly: true, pruneLockfileImporters: true }))

  await mutateModules([
    {
      ...importers[0],
      dependencySelectors: ['@pnpm.e2e/pkg-with-1-dep'],
      mutation: 'installSome',
    },
  ], await testDefaults({ allProjects, hoistPattern: '*' }))

  const rootModules = assertProject(process.cwd())
  const currentLockfile = await rootModules.readCurrentLockfile()
  expect(currentLockfile.importers).toHaveProperty(['project-3'])
  expect(currentLockfile.packages).toHaveProperty(['/@pnpm.e2e/foobar/100.0.0'])
})

test('some projects were removed from the workspace and the ones that are left depend on them', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'project-2': 'workspace:1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
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
  await mutateModules(importers, await testDefaults({ allProjects, workspacePackages }))

  await expect(
    mutateModules(importers.slice(0, 1), await testDefaults({
      allProjects: allProjects.slice(0, 1),
      pruneLockfileImporters: true,
      workspacePackages: pick(['project-1'], workspacePackages),
    } as any)) // eslint-disable-line
  ).rejects.toThrow(/No matching version found for/)
})

test('dependencies of other importers are not pruned when installing for a subset of importers', async () => {
  const projects = preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const [{ manifest }] = await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'project-1',
          version: '1.0.0',

          dependencies: {
            'is-positive': '1.0.0',
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
            'is-negative': '1.0.0',
          },
        },
        rootDir: path.resolve('project-2'),
      },
    ],
  }))

  await addDependenciesToPackage(manifest, ['is-positive@2'], await testDefaults({
    dir: path.resolve('project-1'),
    lockfileDir: process.cwd(),
    modulesCacheMaxAge: 0,
    pruneLockfileImporters: false,
  }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootModules = assertProject(process.cwd())
  await rootModules.has('.pnpm/is-positive@2.0.0')
  await rootModules.hasNot('.pnpm/is-positive@1.0.0')
  await rootModules.has('.pnpm/is-negative@1.0.0')

  const lockfile = await rootModules.readCurrentLockfile()
  expect(Object.keys(lockfile.importers)).toStrictEqual(['project-1', 'project-2'])
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/is-negative/1.0.0',
    '/is-positive/2.0.0',
  ])
})

test('dependencies of other importers are not pruned when (headless) installing for a subset of importers', async () => {
  const projects = preparePackages([
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
          'is-positive': '1.0.0',
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
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2'),
    },
  ]
  const [{ manifest }] = await mutateModules(importers, await testDefaults({ allProjects }))

  await addDependenciesToPackage(manifest, ['is-positive@2'], await testDefaults({
    dir: path.resolve('project-1'),
    lockfileDir: process.cwd(),
    lockfileOnly: true,
    pruneLockfileImporters: false,
  }))
  await mutateModules(importers.slice(0, 1), await testDefaults({
    allProjects,
    frozenLockfile: true,
    modulesCacheMaxAge: 0,
    pruneLockfileImporters: false,
  }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootModules = assertProject(process.cwd())
  await rootModules.has('.pnpm/is-positive@2.0.0')
  await rootModules.hasNot('.pnpm/is-positive@1.0.0')
  await rootModules.has('.pnpm/is-negative@1.0.0')
})

test('adding a new dev dependency to project that uses a shared lockfile', async () => {
  prepareEmpty()

  let [{ manifest }] = await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'project-1',
          version: '1.0.0',

          dependencies: {
            'is-positive': '1.0.0',
          },
        },
        rootDir: path.resolve('project-1'),
      },
    ],
  }))
  manifest = await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], await testDefaults({ prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' }))

  expect(manifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  expect(manifest.devDependencies).toStrictEqual({ 'is-negative': '1.0.0' })
})

test('headless install is used when package linked to another package in the workspace', async () => {
  const pkg1 = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
      'project-2': 'link:../project-2',
    },
  }
  const pkg2 = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages([pkg1, pkg2])

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
      manifest: pkg1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: pkg2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, lockfileOnly: true }))

  const reporter = sinon.spy()
  await mutateModules(importers.slice(0, 1), await testDefaults({ allProjects, reporter }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  await projects['project-1'].has('is-positive')
  await projects['project-1'].has('project-2')
  await projects['project-2'].hasNot('is-negative')
})

test('headless install is used with an up-to-date lockfile when package references another package via workspace: protocol', async () => {
  const pkg1 = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
      'project-2': 'workspace:1.0.0',
    },
  }
  const pkg2 = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages([pkg1, pkg2])

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
      manifest: pkg1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: pkg2,
      rootDir: path.resolve('project-2'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: pkg1,
      },
    },
    'project-2': {
      '1.0.0': {
        dir: path.resolve('project-2'),
        manifest: pkg2,
      },
    },
  }
  await mutateModules(importers, await testDefaults({ allProjects, lockfileOnly: true, workspacePackages }))

  const reporter = sinon.spy()
  await mutateModules(importers, await testDefaults({ allProjects, reporter, workspacePackages }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  await projects['project-1'].has('is-positive')
  await projects['project-1'].has('project-2')
  await projects['project-2'].has('is-negative')
})

test('headless install is used when packages are not linked from the workspace (unless workspace ranges are used)', async () => {
  const foo = {
    name: '@pnpm.e2e/foo',
    version: '1.0.0',

    dependencies: {
      '@pnpm.e2e/qar': 'workspace:*',
    },
  }
  const bar = {
    name: '@pnpm.e2e/bar',
    version: '1.0.0',

    dependencies: {
      '@pnpm.e2e/qar': '100.0.0',
    },
  }
  const qar = {
    name: '@pnpm.e2e/qar',
    version: '100.0.0',
  }
  preparePackages([{ location: 'foo', package: foo }, { location: 'bar', package: bar }, { location: 'qar', package: qar }])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('foo'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('bar'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('qar'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: foo,
      rootDir: path.resolve('foo'),
    },
    {
      buildIndex: 0,
      manifest: bar,
      rootDir: path.resolve('bar'),
    },
    {
      buildIndex: 0,
      manifest: qar,
      rootDir: path.resolve('qar'),
    },
  ]
  const workspacePackages = {
    '@pnpm.e2e/qar': {
      '100.0.0': {
        dir: path.resolve('qar'),
        manifest: qar,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    linkWorkspacePackagesDepth: -1,
    lockfileOnly: true,
    workspacePackages,
  }))

  const reporter = sinon.spy()
  await mutateModules(importers, await testDefaults({
    allProjects,
    linkWorkspacePackagesDepth: -1,
    reporter,
    workspacePackages,
  }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()
})

test('current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile', async () => {
  const pkg1 = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  const pkg2 = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  preparePackages([pkg1, pkg2])

  await mutateModulesInSingleProject({
    manifest: pkg1,
    mutation: 'install',
    rootDir: path.resolve('project-1'),
  }, await testDefaults({ lockfileOnly: true }))

  await mutateModulesInSingleProject({
    manifest: pkg2,
    mutation: 'install',
    rootDir: path.resolve('project-2'),
  }, await testDefaults())

  const currentLockfile = await readCurrentLockfile(path.resolve('node_modules/.pnpm'), { ignoreIncompatible: false })

  expect(Object.keys(currentLockfile?.packages ?? {})).toStrictEqual(['/is-negative/1.0.0'])
})

test('partial installation in a monorepo does not remove dependencies of other workspace projects', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  prepareEmpty()

  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          dependencies: {
            'is-positive': '1.0.0',
          },
        },
        rootDir: path.resolve('project-1'),
      },
      {
        buildIndex: 0,
        manifest: {
          dependencies: {
            '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
          },
        },
        rootDir: path.resolve('project-2'),
      },
    ],
  }))

  await writeYamlFile(path.resolve('pnpm-lock.yaml'), {
    importers: {
      'project-1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
        specifiers: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha512-RWObNQIluSr56fVbOwD75Dt5CE2aiPReTMMUblYEMEqUI+iJw5ovTyO7LzUG/VJ4iVL2uUrbkQ6+rq4z4WOdDw==',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        },
      },
      '/@pnpm.e2e/pkg-with-1-dep/100.0.0': {
        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha512-OStTw86MRiQHB1JTSy6wl+9GT46aK8w4ghZT3e8ZN899J+FUsfD1nFl5gANa4Qol1LTBRqXeKomgXIAo9R/RZA==',
        },
      },
    },
  }, { lineWidth: 1000 })

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        'is-positive': '2.0.0',
      },
    },
    mutation: 'install',
    rootDir: path.resolve('project-1'),
  }, await testDefaults())

  expect(await exists(path.resolve('node_modules/.pnpm/is-positive@2.0.0/node_modules/is-positive'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
})

test('partial installation in a monorepo does not remove dependencies of other workspace projects when lockfile is frozen', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  prepareEmpty()

  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          dependencies: {
            'is-positive': '1.0.0',
          },
        },
        rootDir: path.resolve('project-1'),
      },
      {
        buildIndex: 0,
        manifest: {
          dependencies: {
            '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
          },
        },
        rootDir: path.resolve('project-2'),
      },
    ],
  }))

  await writeYamlFile(path.resolve('pnpm-lock.yaml'), {
    importers: {
      'project-1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
        specifiers: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha512-RWObNQIluSr56fVbOwD75Dt5CE2aiPReTMMUblYEMEqUI+iJw5ovTyO7LzUG/VJ4iVL2uUrbkQ6+rq4z4WOdDw==',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        },
      },
      '/@pnpm.e2e/pkg-with-1-dep/100.0.0': {
        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha512-OStTw86MRiQHB1JTSy6wl+9GT46aK8w4ghZT3e8ZN899J+FUsfD1nFl5gANa4Qol1LTBRqXeKomgXIAo9R/RZA==',
        },
      },
    },
  }, { lineWidth: 1000 })

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    mutation: 'install',
    rootDir: path.resolve('project-1'),
  }, await testDefaults({ frozenLockfile: true }))

  expect(await exists(path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
})

test('adding a new dependency with the workspace: protocol', async () => {
  await addDistTag({ package: 'foo', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  const { manifest } = await mutateModulesInSingleProject({
    dependencySelectors: ['foo'],
    manifest: {
      name: 'project-1',
      version: '1.0.0',
    },
    mutation: 'installSome',
    rootDir: path.resolve('project-1'),
  }, await testDefaults({
    saveWorkspaceProtocol: true,
    workspacePackages: {
      foo: {
        '1.0.0': {
          dir: '',
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
        },
      },
    },
  }))

  expect(manifest.dependencies).toStrictEqual({ foo: 'workspace:^1.0.0' })
})

test('adding a new dependency with the workspace: protocol and save-workspace-protocol is "rolling"', async () => {
  await addDistTag({ package: 'foo', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  const { manifest } = await mutateModulesInSingleProject({
    dependencySelectors: ['foo'],
    manifest: {
      name: 'project-1',
      version: '1.0.0',
    },
    mutation: 'installSome',
    rootDir: path.resolve('project-1'),
  }, await testDefaults({
    saveWorkspaceProtocol: 'rolling',
    workspacePackages: {
      foo: {
        '1.0.0': {
          dir: '',
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
        },
      },
    },
  }))

  expect(manifest.dependencies).toStrictEqual({ foo: 'workspace:^' })
})

test('update workspace range', async () => {
  prepareEmpty()

  const updatedImporters = await mutateModules([
    {
      dependencySelectors: ['dep1', 'dep2', 'dep3', 'dep4', 'dep5', 'dep6'],
      mutation: 'installSome',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
    allProjects: [
      {
        manifest: {
          name: 'project-1',
          version: '1.0.0',

          dependencies: {
            dep1: 'workspace:1.0.0',
            dep2: 'workspace:~1.0.0',
            dep3: 'workspace:^1.0.0',
            dep4: 'workspace:1',
            dep5: 'workspace:1.0',
            dep6: 'workspace:*',
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
            dep1: 'workspace:1.0.0',
            dep2: 'workspace:~1.0.0',
            dep3: 'workspace:^1.0.0',
            dep4: 'workspace:1',
            dep5: 'workspace:1.0',
            dep6: 'workspace:*',
          },
        },
        rootDir: path.resolve('project-2'),
      },
    ],
    update: true,
    workspacePackages: {
      dep1: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep1',
            version: '2.0.0',
          },
        },
      },
      dep2: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep2',
            version: '2.0.0',
          },
        },
      },
      dep3: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep3',
            version: '2.0.0',
          },
        },
      },
      dep4: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep4',
            version: '2.0.0',
          },
        },
      },
      dep5: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep5',
            version: '2.0.0',
          },
        },
      },
      dep6: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep6',
            version: '2.0.0',
          },
        },
      },
    },
  }))

  const expected = {
    dep1: 'workspace:2.0.0',
    dep2: 'workspace:~2.0.0',
    dep3: 'workspace:^2.0.0',
    dep4: 'workspace:^2.0.0',
    dep5: 'workspace:~2.0.0',
    dep6: 'workspace:2.0.0',
  }
  expect(updatedImporters[0].manifest.dependencies).toStrictEqual(expected)
  expect(updatedImporters[1].manifest.dependencies).toStrictEqual(expected)
})

test('update workspace range when save-workspace-protocol is "rolling"', async () => {
  prepareEmpty()

  const updatedImporters = await mutateModules([
    {
      dependencySelectors: ['dep1', 'dep2', 'dep3', 'dep4', 'dep5', 'dep6'],
      mutation: 'installSome',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
    allProjects: [
      {
        manifest: {
          name: 'project-1',
          version: '1.0.0',

          dependencies: {
            dep1: 'workspace:1.0.0',
            dep2: 'workspace:~1.0.0',
            dep3: 'workspace:^1.0.0',
            dep4: 'workspace:1',
            dep5: 'workspace:1.0',
            dep6: 'workspace:*',
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
            dep1: 'workspace:1.0.0',
            dep2: 'workspace:~1.0.0',
            dep3: 'workspace:^1.0.0',
            dep4: 'workspace:1',
            dep5: 'workspace:1.0',
            dep6: 'workspace:*',
          },
        },
        rootDir: path.resolve('project-2'),
      },
    ],
    saveWorkspaceProtocol: 'rolling',
    update: true,
    workspacePackages: {
      dep1: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep1',
            version: '2.0.0',
          },
        },
      },
      dep2: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep2',
            version: '2.0.0',
          },
        },
      },
      dep3: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep3',
            version: '2.0.0',
          },
        },
      },
      dep4: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep4',
            version: '2.0.0',
          },
        },
      },
      dep5: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep5',
            version: '2.0.0',
          },
        },
      },
      dep6: {
        '2.0.0': {
          dir: '',
          manifest: {
            name: 'dep6',
            version: '2.0.0',
          },
        },
      },
    },
  }))

  const expected = {
    dep1: 'workspace:*',
    dep2: 'workspace:~',
    dep3: 'workspace:^',
    dep4: 'workspace:^',
    dep5: 'workspace:~',
    dep6: 'workspace:*',
  }
  expect(updatedImporters[0].manifest.dependencies).toStrictEqual(expected)
  expect(updatedImporters[1].manifest.dependencies).toStrictEqual(expected)
})

test('remove dependencies of a project that was removed from the workspace (during non-headless install)', async () => {
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
          'is-positive': '1.0.0',
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
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects }))

  await mutateModules(importers.slice(0, 1), await testDefaults({ allProjects: allProjects.slice(0, 1), lockfileOnly: true, pruneLockfileImporters: true }))

  const project = assertProject(process.cwd())

  {
    const wantedLockfile = await project.readLockfile()
    expect(Object.keys(wantedLockfile.importers)).toStrictEqual(['project-1'])
    expect(Object.keys(wantedLockfile.packages)).toStrictEqual(['/is-positive/1.0.0'])

    const currentLockfile = await project.readCurrentLockfile()
    expect(Object.keys(currentLockfile.importers)).toStrictEqual(['project-1', 'project-2'])
    expect(Object.keys(currentLockfile.packages)).toStrictEqual(['/is-negative/1.0.0', '/is-positive/1.0.0'])

    await project.has('.pnpm/is-positive@1.0.0')
    await project.has('.pnpm/is-negative@1.0.0')
  }

  await mutateModules(importers.slice(0, 1), await testDefaults({
    allProjects: allProjects.slice(0, 1),
    preferFrozenLockfile: false,
    modulesCacheMaxAge: 0,
  }))
  {
    const currentLockfile = await project.readCurrentLockfile()
    expect(Object.keys(currentLockfile.importers)).toStrictEqual(['project-1'])
    expect(Object.keys(currentLockfile.packages)).toStrictEqual(['/is-positive/1.0.0'])

    await project.has('.pnpm/is-positive@1.0.0')
    await project.hasNot('.pnpm/is-negative@1.0.0')
  }
})

test('do not resolve a subdependency from the workspace by default', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  preparePackages([
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: '@pnpm.e2e/dep-of-pkg-with-1-dep',
      package: { name: '@pnpm.e2e/dep-of-pkg-with-1-dep' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
        version: '100.1.0',
      },
      rootDir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
    },
  ]
  const workspacePackages = {
    '@pnpm.e2e/dep-of-pkg-with-1-dep': {
      '100.1.0': {
        dir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
        manifest: {
          name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
          version: '100.1.0',
        },
      },
    },
  }
  await mutateModules(importers, await testDefaults({ allProjects, workspacePackages }))

  const project = assertProject(process.cwd())

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.1.0')
})

test('resolve a subdependency from the workspace', async () => {
  preparePackages([
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: '@pnpm.e2e/dep-of-pkg-with-1-dep',
      package: { name: '@pnpm.e2e/dep-of-pkg-with-1-dep' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
        version: '100.1.0',
      },
      rootDir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
    },
  ]
  const workspacePackages = {
    '@pnpm.e2e/dep-of-pkg-with-1-dep': {
      '100.1.0': {
        dir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
        manifest: {
          name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
          version: '100.1.0',
        },
      },
    },
  }
  await mutateModules(importers, await testDefaults({ allProjects, linkWorkspacePackagesDepth: Infinity, workspacePackages }))

  const project = assertProject(process.cwd())

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('link:@pnpm.e2e/dep-of-pkg-with-1-dep')

  await rimraf('node_modules')

  // Testing that headless installation does not fail with links in subdeps
  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
    workspacePackages,
  }))
})

test('resolve a subdependency from the workspace and use it as a peer', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.1', distTag: 'latest' })
  preparePackages([
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: '@pnpm.e2e/peer-a',
      package: { name: '@pnpm.e2e/peer-a' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('@pnpm.e2e/peer-a'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/abc-grand-parent-with-c': '1.0.0',
          '@pnpm.e2e/abc-parent-with-ab': '1.0.0',
        },
      },
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: '@pnpm.e2e/peer-a',
        version: '1.0.1',
      },
      rootDir: path.resolve('@pnpm.e2e/peer-a'),
    },
  ]
  const workspacePackages = {
    '@pnpm.e2e/peer-a': {
      '1.0.1': {
        dir: path.resolve('@pnpm.e2e/peer-a'),
        manifest: {
          name: '@pnpm.e2e/peer-a',
          version: '1.0.1',
        },
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    linkWorkspacePackagesDepth: Infinity,
    strictPeerDependencies: false,
    workspacePackages,
  }))

  const project = assertProject(process.cwd())

  const wantedLockfile = await project.readLockfile()
  const suffix1 = createPeersFolderSuffix([{ name: '@pnpm.e2e/peer-a', version: '@pnpm.e2e+peer-a' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }])
  const suffix2 = createPeersFolderSuffix([{ name: '@pnpm.e2e/peer-a', version: '@pnpm.e2e+peer-a' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.1' }])
  expect(Object.keys(wantedLockfile.packages).sort()).toStrictEqual(
    [
      '/@pnpm.e2e/abc-grand-parent-with-c/1.0.0',
      '/@pnpm.e2e/abc-parent-with-ab/1.0.0',
      '/@pnpm.e2e/abc-parent-with-ab/1.0.0_@pnpm.e2e+peer-c@1.0.1',
      `/@pnpm.e2e/abc/1.0.0${suffix1}`,
      `/@pnpm.e2e/abc/1.0.0${suffix2}`,
      '/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0',
      '/is-positive/1.0.0',
      '/@pnpm.e2e/peer-b/1.0.0',
      '/@pnpm.e2e/peer-c/1.0.1',
    ].sort()
  )
  expect(wantedLockfile.packages['/@pnpm.e2e/abc-parent-with-ab/1.0.0'].dependencies?.['@pnpm.e2e/peer-a']).toBe('link:@pnpm.e2e/peer-a')
  expect(wantedLockfile.packages[`/@pnpm.e2e/abc/1.0.0${suffix1}`].dependencies?.['@pnpm.e2e/peer-a']).toBe('link:@pnpm.e2e/peer-a')
})

test('resolve a subdependency from the workspace, when it uses the workspace protocol', async () => {
  preparePackages([
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: '@pnpm.e2e/dep-of-pkg-with-1-dep',
      package: { name: '@pnpm.e2e/dep-of-pkg-with-1-dep' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
        version: '100.1.0',
      },
      rootDir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
    },
  ]
  const workspacePackages = {
    '@pnpm.e2e/dep-of-pkg-with-1-dep': {
      '100.1.0': {
        dir: path.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'),
        manifest: {
          name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
          version: '100.1.0',
        },
      },
    },
  }
  const overrides = {
    '@pnpm.e2e/dep-of-pkg-with-1-dep': 'workspace:*',
  }
  await mutateModules(importers, await testDefaults({
    allProjects,
    linkWorkspacePackagesDepth: -1,
    overrides,
    workspacePackages,
  }))

  const project = assertProject(process.cwd())

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile.packages['/@pnpm.e2e/pkg-with-1-dep/100.0.0'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('link:@pnpm.e2e/dep-of-pkg-with-1-dep')

  await rimraf('node_modules')

  // Testing that headless installation does not fail with links in subdeps
  await mutateModules(importers, await testDefaults({
    allProjects,
    frozenLockfile: true,
    overrides,
    workspacePackages,
  }))
})

test('install the dependency that is already present in the workspace when adding a new direct dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest1: ProjectManifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
    },
  }
  const manifest2: ProjectManifest = { name: 'project-2' }

  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
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
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects }))

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await mutateModules([
    importers[0],
    {
      ...importers[1],
      dependencySelectors: ['@pnpm.e2e/dep-of-pkg-with-1-dep'],
      mutation: 'installSome',
    },
  ], await testDefaults({
    allProjects,
    lockfileDir: process.cwd(),
    workspacePackages: {
      'project-1': {
        '1.0.0': {
          dir: path.resolve('project-1'),
          manifest: manifest1,
        },
      },
    },
  }))

  const rootModules = assertProject(process.cwd())
  const currentLockfile = await rootModules.readCurrentLockfile()

  expect(currentLockfile.importers['project-1'].specifiers?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('^100.0.0')
  expect(currentLockfile.importers['project-2'].specifiers?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('^100.0.0')

  expect(currentLockfile.importers['project-1'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  expect(currentLockfile.importers['project-2'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
})

test('do not update dependency that has the same name as a dependency in the workspace', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const manifest1: ProjectManifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
    },
  }
  const manifest2: ProjectManifest = { name: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0' }

  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
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
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  const workspacePackages = {
    'project-1': {
      '1.0.0': {
        dir: path.resolve('project-1'),
        manifest: manifest1,
      },
    },
    '@pnpm.e2e/dep-of-pkg-with-1-dep': {
      '100.1.0': {
        dir: path.resolve('project-2'),
        manifest: manifest2,
      },
    },
  }
  await mutateModules(importers, await testDefaults({ allProjects, linkWorkspacePackagesDepth: -1, workspacePackages }))
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await mutateModules([
    {
      ...importers[0],
      dependencySelectors: ['is-negative@2.1.0'],
      mutation: 'installSome',
    },
    importers[1],
  ], await testDefaults({ allProjects, linkWorkspacePackagesDepth: -1, workspacePackages, preferredVersions: {} }))

  const rootModules = assertProject(process.cwd())
  const currentLockfile = await rootModules.readCurrentLockfile()
  expect(Object.keys(currentLockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0',
    '/is-negative/2.1.0',
  ])
})

test('symlink local package from the location described in its publishConfig.directory when linkDirectory is true', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-1/dist',
      package: { name: 'project-1-dist' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    publishConfig: {
      directory: 'dist',
      linkDirectory: true,
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'project-1': 'workspace:*',
    },
  }
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
  await mutateModules(importers, await testDefaults({ allProjects, workspacePackages }))

  {
    const linkedManifest = await loadJsonFile<{ name: string }>('project-2/node_modules/project-1/package.json')
    expect(linkedManifest.name).toBe('project-1-dist')
  }

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.importers['project-1'].publishDirectory).toBe('dist')

  await rimraf('node_modules')
  await mutateModules(importers, await testDefaults({ allProjects, frozenLockfile: true, workspacePackages }))

  {
    const linkedManifest = await loadJsonFile<{ name: string }>('project-2/node_modules/project-1/package.json')
    expect(linkedManifest.name).toBe('project-1-dist')
  }
})

test('do not symlink local package from the location described in its publishConfig.directory', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-1/dist',
      package: { name: 'project-1-dist' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    publishConfig: {
      directory: 'dist',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'project-1': 'workspace:*',
    },
  }
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
  await mutateModules(importers, await testDefaults({ allProjects, workspacePackages }))

  const linkedManifest = await loadJsonFile<{ name: string }>('project-2/node_modules/project-1/package.json')
  expect(linkedManifest.name).toBe('project-1')
})
