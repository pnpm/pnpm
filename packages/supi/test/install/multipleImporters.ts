import assertProject from '@pnpm/assert-project'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readCurrentLockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  MutatedProject,
  mutateModules,
} from 'supi'
import promisifyTape from 'tape-promise'
import { addDistTag, testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import exists = require('path-exists')
import R = require('ramda')
import sinon = require('sinon')
import tape = require('tape')
import writeYamlFile = require('write-yaml-file')

const test = promisifyTape(tape)

test('install only the dependencies of the specified importer', async (t) => {
  const projects = preparePackages(t, [
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
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ lockfileOnly: true }))

  await mutateModules(importers.slice(0, 1), await testDefaults())

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')

  const rootModules = assertProject(t, process.cwd())
  await rootModules.has('.pnpm/is-positive@1.0.0')
  await rootModules.hasNot('.pnpm/is-negative@1.0.0')
})

test('install only the dependencies of the specified importer. The current lockfile has importers that do not exist anymore', async (t) => {
  preparePackages(t, [
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
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-3',
        version: '1.0.0',

        dependencies: {
          foobar: '100.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  await mutateModules(importers, await testDefaults({ hoistPattern: '*' }))
  await mutateModules(importers.slice(0, 2), await testDefaults({ lockfileOnly: true, pruneLockfileImporters: true }))

  await mutateModules([
    {
      ...importers[0],
      dependencySelectors: ['pkg-with-1-dep'],
      mutation: 'installSome',
    },
  ], await testDefaults({ hoistPattern: '*' }))

  const rootModules = assertProject(t, process.cwd())
  const currentLockfile = await rootModules.readCurrentLockfile()
  t.ok(currentLockfile.importers['project-3'])
  t.ok(currentLockfile.packages['/foobar/100.0.0'])
})

test('some projects were removed from the workspace and the ones that are left depend on them', async (t) => {
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
  preparePackages(t, [
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
      buildIndex: 0,
      manifest: project1Manifest,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      mutation: 'install',
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
  await mutateModules(importers, await testDefaults({ workspacePackages }))

  let err!: PnpmError
  try {
    await mutateModules([importers[0]], await testDefaults({
      pruneLockfileImporters: true,
      workspacePackages: R.pick(['project-1'], workspacePackages),
    } as any)) // eslint-disable-line
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE')
})

test('dependencies of other importers are not pruned when installing for a subset of importers', async (t) => {
  const projects = preparePackages(t, [
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
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults())

  await addDependenciesToPackage(manifest, ['is-positive@2'], await testDefaults({
    dir: path.resolve('project-1'),
    lockfileDir: process.cwd(),
  }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootModules = assertProject(t, process.cwd())
  await rootModules.has('.pnpm/is-positive@2.0.0')
  await rootModules.hasNot('.pnpm/is-positive@1.0.0')
  await rootModules.has('.pnpm/is-negative@1.0.0')

  const lockfile = await rootModules.readCurrentLockfile()
  t.deepEqual(Object.keys(lockfile.importers), ['project-1', 'project-2'])
  t.deepEqual(Object.keys(lockfile.packages), [
    '/is-negative/1.0.0',
    '/is-positive/2.0.0',
  ], `packages of importer that was not selected by last installation are not removed from current ${WANTED_LOCKFILE}`)
})

test('dependencies of other importers are not pruned when (headless) installing for a subset of importers', async (t) => {
  const projects = preparePackages(t, [
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
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  const [{ manifest }] = await mutateModules(importers, await testDefaults())

  await addDependenciesToPackage(manifest, ['is-positive@2'], await testDefaults({
    dir: path.resolve('project-1'),
    lockfileDir: process.cwd(),
    lockfileOnly: true,
  }))
  await mutateModules(importers.slice(0, 1), await testDefaults({ frozenLockfile: true }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootModules = assertProject(t, process.cwd())
  await rootModules.has('.pnpm/is-positive@2.0.0')
  await rootModules.hasNot('.pnpm/is-positive@1.0.0')
  await rootModules.has('.pnpm/is-negative@1.0.0')
})

test('adding a new dev dependency to project that uses a shared lockfile', async (t) => {
  prepareEmpty(t)

  let [{ manifest }] = await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults())
  manifest = await addDependenciesToPackage(manifest, ['is-negative@1.0.0'], await testDefaults({ prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' }))

  t.deepEqual(manifest.dependencies, { 'is-positive': '1.0.0' }, 'prod deps unchanged in package.json')
  t.deepEqual(manifest.devDependencies, { 'is-negative': '1.0.0' }, 'dev deps have a new dependency in package.json')
})

test('headless install is used when package linked to another package in the workspace', async (t) => {
  const pkg1 = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
      'project-2': 'file:../project-2',
    },
  }
  const pkg2 = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages(t, [pkg1, pkg2])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: pkg1,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: pkg2,
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ lockfileOnly: true }))

  const reporter = sinon.spy()
  await mutateModules(importers.slice(0, 1), await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up-to-date, resolution step is skipped',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await projects['project-1'].has('is-positive')
  await projects['project-1'].has('project-2')
  await projects['project-2'].hasNot('is-negative')
})

test('headless install is used with an up-to-date lockfile when package references another package via workspace: protocol', async (t) => {
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
  const projects = preparePackages(t, [pkg1, pkg2])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: pkg1,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: pkg2,
      mutation: 'install',
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
  await mutateModules(importers, await testDefaults({ lockfileOnly: true, workspacePackages }))

  const reporter = sinon.spy()
  await mutateModules(importers, await testDefaults({ reporter, workspacePackages }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up-to-date, resolution step is skipped',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await projects['project-1'].has('is-positive')
  await projects['project-1'].has('project-2')
  await projects['project-2'].has('is-negative')
})

test('headless install is used when packages are not linked from the workspace (unless workspace ranges are used)', async (t) => {
  const foo = {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      qar: 'workspace:*',
    },
  }
  const bar = {
    name: 'bar',
    version: '1.0.0',

    dependencies: {
      qar: '100.0.0',
    },
  }
  const qar = {
    name: 'qar',
    version: '100.0.0',
  }
  preparePackages(t, [foo, bar, qar])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: foo,
      mutation: 'install',
      rootDir: path.resolve('foo'),
    },
    {
      buildIndex: 0,
      manifest: bar,
      mutation: 'install',
      rootDir: path.resolve('bar'),
    },
    {
      buildIndex: 0,
      manifest: qar,
      mutation: 'install',
      rootDir: path.resolve('qar'),
    },
  ]
  const workspacePackages = {
    qar: {
      '100.0.0': {
        dir: path.resolve('qar'),
        manifest: qar,
      },
    },
  }
  await mutateModules(importers, await testDefaults({
    linkWorkspacePackagesDepth: -1,
    lockfileOnly: true,
    workspacePackages,
  }))

  const reporter = sinon.spy()
  await mutateModules(importers, await testDefaults({
    linkWorkspacePackagesDepth: -1,
    reporter,
    workspacePackages,
  }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up-to-date, resolution step is skipped',
    name: 'pnpm',
  }), 'start of headless installation logged')
})

test('current lockfile contains only installed dependencies when adding a new importer to workspace with shared lockfile', async (t) => {
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
  preparePackages(t, [pkg1, pkg2])

  await mutateModules([
    {
      buildIndex: 0,
      manifest: pkg1,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults({ lockfileOnly: true }))

  await mutateModules([
    {
      buildIndex: 0,
      manifest: pkg2,
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults())

  const currentLockfile = await readCurrentLockfile(path.resolve('node_modules/.pnpm'), { ignoreIncompatible: false })

  t.deepEqual(Object.keys(currentLockfile?.packages ?? {}), ['/is-negative/1.0.0'])
})

test('partial installation in a monorepo does not remove dependencies of other workspace projects', async (t: tape.Test) => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  prepareEmpty(t)

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults())

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
          'pkg-with-1-dep': '100.0.0',
        },
        specifiers: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/dep-of-pkg-with-1-dep/100.0.0': {
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
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        },
      },
      '/pkg-with-1-dep/100.0.0': {
        dependencies: {
          'dep-of-pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha512-OStTw86MRiQHB1JTSy6wl+9GT46aK8w4ghZT3e8ZN899J+FUsfD1nFl5gANa4Qol1LTBRqXeKomgXIAo9R/RZA==',
        },
      },
    },
  })

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'is-positive': '2.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults())

  t.ok(await exists(path.resolve('node_modules/.pnpm/is-positive@2.0.0/node_modules/is-positive')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/pkg-with-1-dep@100.0.0/node_modules/pkg-with-1-dep')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/dep-of-pkg-with-1-dep@100.1.0/node_modules/dep-of-pkg-with-1-dep')))
})

test('partial installation in a monorepo does not remove dependencies of other workspace projects when lockfile is frozen', async (t: tape.Test) => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  prepareEmpty(t)

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults())

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
          'pkg-with-1-dep': '100.0.0',
        },
        specifiers: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/dep-of-pkg-with-1-dep/100.0.0': {
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
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        },
      },
      '/pkg-with-1-dep/100.0.0': {
        dependencies: {
          'dep-of-pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha512-OStTw86MRiQHB1JTSy6wl+9GT46aK8w4ghZT3e8ZN899J+FUsfD1nFl5gANa4Qol1LTBRqXeKomgXIAo9R/RZA==',
        },
      },
    },
  })

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults({ frozenLockfile: true }))

  t.ok(await exists(path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/pkg-with-1-dep@100.0.0/node_modules/pkg-with-1-dep')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/dep-of-pkg-with-1-dep@100.1.0/node_modules/dep-of-pkg-with-1-dep')))
})

test('adding a new dependency with the workspace: protocol', async (t) => {
  await addDistTag('foo', '1.0.0', 'latest')
  prepareEmpty(t)

  const [{ manifest }] = await mutateModules([
    {
      dependencySelectors: ['foo'],
      manifest: {
        name: 'project-1',
        version: '1.0.0',
      },
      mutation: 'installSome',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults({
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

  t.deepEqual(manifest.dependencies, { foo: 'workspace:^1.0.0' })
})

test('update workspace range', async (t) => {
  prepareEmpty(t)

  const updatedImporters = await mutateModules([
    {
      dependencySelectors: ['dep1', 'dep2', 'dep3', 'dep4', 'dep5', 'dep6'],
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
      mutation: 'installSome',
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({
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
    dep6: 'workspace:*',
  }
  t.deepEqual(updatedImporters[0].manifest.dependencies, expected)
  t.deepEqual(updatedImporters[1].manifest.dependencies, expected)
})

test('remove dependencies of a project that was removed from the workspace (during non-headless install)', async (t) => {
  preparePackages(t, [
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
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults())

  await mutateModules(importers.slice(0, 1), await testDefaults({ lockfileOnly: true, pruneLockfileImporters: true }))

  const project = assertProject(t, process.cwd())

  {
    const wantedLockfile = await project.readLockfile()
    t.deepEqual(Object.keys(wantedLockfile.importers), ['project-1'])
    t.deepEqual(Object.keys(wantedLockfile.packages), ['/is-positive/1.0.0'])

    const currentLockfile = await project.readCurrentLockfile()
    t.deepEqual(Object.keys(currentLockfile.importers), ['project-1', 'project-2'])
    t.deepEqual(Object.keys(currentLockfile.packages), ['/is-negative/1.0.0', '/is-positive/1.0.0'])

    await project.has('.pnpm/is-positive@1.0.0')
    await project.has('.pnpm/is-negative@1.0.0')
  }

  await mutateModules(importers.slice(0, 1), await testDefaults({ preferFrozenLockfile: false }))
  {
    const currentLockfile = await project.readCurrentLockfile()
    t.deepEqual(Object.keys(currentLockfile.importers), ['project-1'])
    t.deepEqual(Object.keys(currentLockfile.packages), ['/is-positive/1.0.0'])

    await project.has('.pnpm/is-positive@1.0.0')
    await project.hasNot('.pnpm/is-negative@1.0.0')
  }
})

test('do not resolve a subdependency from the workspace by default', async (t) => {
  preparePackages(t, [
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: 'dep-of-pkg-with-1-dep',
      package: { name: 'dep-of-pkg-with-1-dep' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'dep-of-pkg-with-1-dep',
        version: '100.1.0',
      },
      mutation: 'install',
      rootDir: path.resolve('dep-of-pkg-with-1-dep'),
    },
  ]
  const workspacePackages = {
    'dep-of-pkg-with-1-dep': {
      '100.1.0': {
        dir: path.resolve('dep-of-pkg-with-1-dep'),
        manifest: {
          name: 'dep-of-pkg-with-1-dep',
          version: '100.1.0',
        },
      },
    },
  }
  await mutateModules(importers, await testDefaults({ workspacePackages }))

  const project = assertProject(t, process.cwd())

  const wantedLockfile = await project.readLockfile()
  t.equal(wantedLockfile.packages['/pkg-with-1-dep/100.0.0'].dependencies['dep-of-pkg-with-1-dep'], '100.1.0')
})

test('resolve a subdependency from the workspace', async (t) => {
  preparePackages(t, [
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: 'dep-of-pkg-with-1-dep',
      package: { name: 'dep-of-pkg-with-1-dep' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'dep-of-pkg-with-1-dep',
        version: '100.1.0',
      },
      mutation: 'install',
      rootDir: path.resolve('dep-of-pkg-with-1-dep'),
    },
  ]
  const workspacePackages = {
    'dep-of-pkg-with-1-dep': {
      '100.1.0': {
        dir: path.resolve('dep-of-pkg-with-1-dep'),
        manifest: {
          name: 'dep-of-pkg-with-1-dep',
          version: '100.1.0',
        },
      },
    },
  }
  await mutateModules(importers, await testDefaults({ linkWorkspacePackagesDepth: Infinity, workspacePackages }))

  const project = assertProject(t, process.cwd())

  const wantedLockfile = await project.readLockfile()
  t.equal(wantedLockfile.packages['/pkg-with-1-dep/100.0.0'].dependencies['dep-of-pkg-with-1-dep'], 'link:dep-of-pkg-with-1-dep')

  await rimraf('node_modules')

  // Testing that headless installation does not fail with links in subdeps
  await mutateModules(importers, await testDefaults({
    frozenLockfile: true,
    workspacePackages,
  }))
})

test('resolve a subdependency from the workspace and use it as a peer', async (t) => {
  await addDistTag('peer-c', '1.0.1', 'latest')
  preparePackages(t, [
    {
      location: 'project',
      package: { name: 'project' },
    },
    {
      location: 'peer-a',
      package: { name: 'peer-a' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project',
        version: '1.0.0',

        dependencies: {
          'abc-grand-parent-with-c': '1.0.0',
          'abc-parent-with-ab': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'peer-a',
        version: '1.0.1',
      },
      mutation: 'install',
      rootDir: path.resolve('peer-a'),
    },
  ]
  const workspacePackages = {
    'peer-a': {
      '1.0.1': {
        dir: path.resolve('peer-a'),
        manifest: {
          name: 'peer-a',
          version: '1.0.1',
        },
      },
    },
  }
  await mutateModules(importers, await testDefaults({ linkWorkspacePackagesDepth: Infinity, workspacePackages }))

  const project = assertProject(t, process.cwd())

  const wantedLockfile = await project.readLockfile()
  t.deepEqual(
    Object.keys(wantedLockfile.packages),
    [
      '/abc-grand-parent-with-c/1.0.0',
      '/abc-parent-with-ab/1.0.0',
      '/abc-parent-with-ab/1.0.0_peer-c@1.0.1',
      '/abc/1.0.0_20890f3ae006d9839e924c7177030952',
      '/abc/1.0.0_peer-a@1.0.1+peer-b@1.0.0',
      '/dep-of-pkg-with-1-dep/100.0.0',
      '/is-positive/1.0.0',
      '/peer-b/1.0.0',
      '/peer-c/1.0.1',
    ]
  )
  t.equal(wantedLockfile.packages['/abc-parent-with-ab/1.0.0'].dependencies['peer-a'], 'link:peer-a')
  t.equal(wantedLockfile.packages['/abc/1.0.0_peer-a@1.0.1+peer-b@1.0.0'].dependencies['peer-a'], 'link:peer-a')
})
