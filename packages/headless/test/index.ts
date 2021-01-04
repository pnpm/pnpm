/// <reference path="../../../typings/index.d.ts" />
import assertProject from '@pnpm/assert-project'
import { ENGINE_NAME, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  PackageManifestLog,
  RootLog,
  StageLog,
  StatsLog,
} from '@pnpm/core-loggers'
import headless from '@pnpm/headless'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import readprojectsContext from '@pnpm/read-projects-context'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { copyFixture } from '@pnpm/test-fixtures'
import testDefaults from './utils/testDefaults'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import isWindows = require('is-windows')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import exists = require('path-exists')
import sinon = require('sinon')
import tempy = require('tempy')
import writeJsonFile = require('write-json-file')

const fixtures = path.join(__dirname, 'fixtures')
const skipOnWindows = isWindows() ? test.skip : test

test('installing a simple project', async () => {
  const prefix = path.join(fixtures, 'simple')
  const reporter = sinon.spy()

  await headless(await testDefaults({
    lockfileDir: prefix,
    reporter,
  }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()

  await project.has('.pnpm/colors@1.2.0')

  await project.isExecutable('.bin/rimraf')

  expect(await project.readCurrentLockfile()).toBeTruthy()
  expect(await project.readModulesManifest()).toBeTruthy()

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: 15,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    requester: prefix,
    status: 'resolved',
  })).toBeTruthy()
})

test('installing only prod deps', async () => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  await project.has('is-positive')
  await project.has('rimraf')
  await project.hasNot('is-negative')
  await project.hasNot('colors')

  await project.isExecutable('.bin/rimraf')
})

test('installing only dev deps', async () => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  await project.hasNot('is-positive')
  await project.hasNot('rimraf')
  await project.has('is-negative')
  await project.hasNot('colors')
})

test('installing non-prod deps then all deps', async () => {
  const prefix = path.join(fixtures, 'prod-dep-is-dev-subdep')

  await headless(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  const inflight = project.requireModule('inflight')
  expect(typeof inflight).toBe('function')

  await project.hasNot('once')

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/is-positive/1.0.0'].dev === false).toBeTruthy()
  }

  {
    const currentLockfile = await project.readCurrentLockfile()
    expect(currentLockfile.packages).not.toHaveProperty(['/is-positive/1.0.0'])
  }

  const reporter = sinon.spy()

  // Repeat normal installation adds missing deps to node_modules
  await headless(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'once',
      realName: 'once',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'inflight',
      realName: 'inflight',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeFalsy()

  await project.has('once')

  {
    const currentLockfile = await project.readCurrentLockfile()
    expect(currentLockfile.packages).toHaveProperty(['/is-positive/1.0.0'])
  }
})

test('installing only optional deps', async () => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    development: false,
    include: {
      dependencies: false,
      devDependencies: false,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
    optional: true,
    production: false,
  }))

  const project = assertProject(prefix)
  await project.hasNot('is-positive')
  await project.hasNot('rimraf')
  await project.hasNot('is-negative')
  await project.has('colors')
})

// Covers https://github.com/pnpm/pnpm/issues/1958
test('not installing optional deps', async () => {
  const prefix = path.join(fixtures, 'simple-with-optional-dep')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(prefix)
  await project.hasNot('is-positive')
  await project.has('pkg-with-good-optional')
})

test('run pre/postinstall scripts', async () => {
  const prefix = path.join(fixtures, 'deps-have-lifecycle-scripts')
  const outputJsonPath = path.join(prefix, 'output.json')
  await rimraf(outputJsonPath)

  await headless(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(prefix)
  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  expect(typeof generatedByPreinstall).toBe('function')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  expect(typeof generatedByPostinstall).toBe('function')

  expect(require(outputJsonPath)).toStrictEqual(['install', 'postinstall']) // eslint-disable-line

  await rimraf(outputJsonPath)
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({ lockfileDir: prefix, ignoreScripts: true }))

  expect(await exists(outputJsonPath)).toBeFalsy()

  const nmPath = path.join(prefix, 'node_modules')
  const modulesYaml = await readModulesYaml(nmPath)
  expect(modulesYaml).toBeTruthy()
  expect(modulesYaml!.pendingBuilds).toStrictEqual(['.', '/pre-and-postinstall-scripts-example/1.0.0'])
})

test('orphan packages are removed', async () => {
  const projectDir = tempy.directory()

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const simpleWithMoreDepsDir = path.join(fixtures, 'simple-with-more-deps')
  const simpleDir = path.join(fixtures, 'simple')
  await fs.copyFile(path.join(simpleWithMoreDepsDir, 'package.json'), destPackageJsonPath)
  await fs.copyFile(path.join(simpleWithMoreDepsDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headless(await testDefaults({
    lockfileDir: projectDir,
  }))

  await fs.copyFile(path.join(simpleDir, 'package.json'), destPackageJsonPath)
  await fs.copyFile(path.join(simpleDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({
    lockfileDir: projectDir,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: projectDir,
    removed: 1,
  } as StatsLog)).toBeTruthy()

  const project = assertProject(projectDir)
  await project.hasNot('resolve-from')
  await project.has('rimraf')
  await project.has('is-negative')
  await project.has('colors')
})

test('available packages are used when node_modules is not clean', async () => {
  const projectDir = tempy.directory()

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const hasGlobDir = path.join(fixtures, 'has-glob')
  const hasGlobAndRimrafDir = path.join(fixtures, 'has-glob-and-rimraf')
  await fs.copyFile(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  await fs.copyFile(path.join(hasGlobDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headless(await testDefaults({ lockfileDir: projectDir }))

  await fs.copyFile(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  await fs.copyFile(path.join(hasGlobAndRimrafDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({ lockfileDir: projectDir, reporter }))

  const project = assertProject(projectDir)
  await project.has('rimraf')
  await project.has('glob')

  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/balanced-match/1.0.0`,
    requester: projectDir,
    status: 'resolved',
  })).toBeFalsy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/rimraf/2.6.2`,
    requester: projectDir,
    status: 'resolved',
  })).toBeTruthy()
})

test('available packages are relinked during forced install', async () => {
  const projectDir = tempy.directory()

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const hasGlobDir = path.join(fixtures, 'has-glob')
  const hasGlobAndRimrafDir = path.join(fixtures, 'has-glob-and-rimraf')
  await fs.copyFile(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  await fs.copyFile(path.join(hasGlobDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headless(await testDefaults({ lockfileDir: projectDir }))

  await fs.copyFile(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  await fs.copyFile(path.join(hasGlobAndRimrafDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({ lockfileDir: projectDir, reporter, force: true }))

  const project = assertProject(projectDir)
  await project.has('rimraf')
  await project.has('glob')

  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/balanced-match/1.0.0`,
    requester: projectDir,
    status: 'resolved',
  })).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/rimraf/2.6.2`,
    requester: projectDir,
    status: 'resolved',
  })).toBeTruthy()
})

test(`fail when ${WANTED_LOCKFILE} is not up-to-date with package.json`, async () => {
  const projectDir = tempy.directory()

  const simpleDir = path.join(fixtures, 'simple')
  await fs.copyFile(path.join(simpleDir, 'package.json'), path.join(projectDir, 'package.json'))

  const simpleWithMoreDepsDir = path.join(fixtures, 'simple-with-more-deps')
  await fs.copyFile(path.join(simpleWithMoreDepsDir, WANTED_LOCKFILE), path.join(projectDir, WANTED_LOCKFILE))

  try {
    await headless(await testDefaults({ lockfileDir: projectDir }))
    throw new Error()
  } catch (err) {
    expect(err.message).toBe(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with package.json`)
  }
})

test('installing local dependency', async () => {
  const prefix = path.join(fixtures, 'has-local-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter }))

  const project = assertProject(prefix)
  expect(project.requireModule('tar-pkg'))
})

test('installing local directory dependency', async () => {
  const prefix = path.join(fixtures, 'has-local-dir-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter }))

  const project = assertProject(prefix)
  expect(project.requireModule('example/package.json')).toBeTruthy()
})

test('installing using passed in lockfile files', async () => {
  const prefix = tempy.directory()

  const simplePkgPath = path.join(fixtures, 'simple')
  await fs.copyFile(path.join(simplePkgPath, 'package.json'), path.join(prefix, 'package.json'))
  await fs.copyFile(path.join(simplePkgPath, WANTED_LOCKFILE), path.join(prefix, WANTED_LOCKFILE))

  const wantedLockfile = await readWantedLockfile(simplePkgPath, { ignoreIncompatible: false })

  await headless(await testDefaults({
    lockfileDir: prefix,
    wantedLockfile,
  }))

  const project = assertProject(prefix)

  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()
})

test('installation of a dependency that has a resolved peer in subdeps', async () => {
  const prefix = path.join(fixtures, 'resolved-peer-deps-in-subdeps')

  await headless(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(prefix)
  expect(project.requireModule('pnpm-default-reporter')).toBeTruthy()
})

test('installing with hoistPattern=*', async () => {
  const prefix = path.join(fixtures, 'simple-shamefully-flatten')
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter, hoistPattern: '*' }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('.pnpm/node_modules/glob')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()

  await project.has('.pnpm/colors@1.2.0')

  await project.isExecutable('.bin/rimraf')
  await project.isExecutable('.pnpm/node_modules/.bin/hello-world-js-bin')

  expect(await project.readCurrentLockfile()).toBeTruthy()
  expect(await project.readModulesManifest()).toBeTruthy()

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: 17,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    requester: prefix,
    status: 'resolved',
  })).toBeTruthy()

  const modules = await project.readModulesManifest()

  expect(modules!.hoistedDependencies['/balanced-match/1.0.0']).toStrictEqual({ 'balanced-match': 'private' })
})

test('installing with publicHoistPattern=*', async () => {
  const prefix = path.join(fixtures, 'simple-shamefully-flatten')
  await rimraf(path.join(prefix, 'node_modules'))
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter, publicHoistPattern: '*' }))

  const project = assertProject(prefix)
  expect(project.requireModule('is-positive')).toBeTruthy()
  expect(project.requireModule('rimraf')).toBeTruthy()
  expect(project.requireModule('glob')).toBeTruthy()
  expect(project.requireModule('is-negative')).toBeTruthy()
  expect(project.requireModule('colors')).toBeTruthy()

  await project.has('.pnpm/colors@1.2.0')

  await project.isExecutable('.bin/rimraf')
  await project.isExecutable('.bin/hello-world-js-bin')

  expect(await project.readCurrentLockfile()).toBeTruthy()
  expect(await project.readModulesManifest()).toBeTruthy()

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: 17,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    requester: prefix,
    status: 'resolved',
  })).toBeTruthy()

  const modules = await project.readModulesManifest()

  expect(modules!.hoistedDependencies['/balanced-match/1.0.0']).toStrictEqual({ 'balanced-match': 'public' })
})

test('installing with publicHoistPattern=* in a project with external lockfile', async () => {
  const lockfileDir = tempy.directory()
  await copyFixture('pkg-with-external-lockfile', lockfileDir)
  const prefix = path.join(lockfileDir, 'pkg')

  let { projects } = await readprojectsContext(
    [
      {
        rootDir: prefix,
      },
    ],
    { lockfileDir }
  )

  projects = await Promise.all(
    projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) }))
  )

  await headless(await testDefaults({
    lockfileDir,
    projects,
    publicHoistPattern: '*',
  }))

  const project = assertProject(lockfileDir)
  expect(project.requireModule('accepts')).toBeTruthy()
})

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

skipOnWindows('using side effects cache', async () => {
  const prefix = path.join(fixtures, 'side-effects')

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    lockfileDir: prefix,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headless(opts)

  const cacheIntegrityPath = path.join(opts.storeDir, 'files/10/0c9ac65f21cb83e1d3b9339731937e96d930d0000075d266d3443307659d27759e81f3bc0e87b202ade1f10c4af6845d060b4a985ee6b3ccc4de163a3d2171-index.json')
  const cacheIntegrity = await loadJsonFile(cacheIntegrityPath)
  expect(cacheIntegrity!['sideEffects']).toBeTruthy()
  expect(cacheIntegrity).toHaveProperty(['sideEffects', ENGINE_NAME, 'build/Makefile'])
  delete cacheIntegrity!['sideEffects'][ENGINE_NAME]['build/Makefile']

  expect(cacheIntegrity).toHaveProperty(['sideEffects', ENGINE_NAME, 'build/binding.Makefile'])
  await writeJsonFile(cacheIntegrityPath, cacheIntegrity)

  await rimraf(path.join(prefix, 'node_modules'))
  const opts2 = await testDefaults({
    lockfileDir: prefix,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    storeDir: opts.storeDir,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headless(opts2)

  expect(await exists(path.join(prefix, 'node_modules/diskusage/build/Makefile'))).toBeFalsy()
  expect(await exists(path.join(prefix, 'node_modules/diskusage/build/binding.Makefile'))).toBeTruthy()
})

test.skip('using side effects cache and hoistPattern=*', async () => {
  const lockfileDir = path.join(fixtures, 'side-effects-of-subdep')

  const { projects } = await readprojectsContext(
    [
      {
        rootDir: lockfileDir,
      },
    ],
    { lockfileDir }
  )

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    hoistPattern: '*',
    lockfileDir,
    projects: await Promise.all(
      projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) }))
    ),
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headless(opts)

  const project = assertProject(lockfileDir)
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage@1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  fs.writeFileSync(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf(path.join(lockfileDir, 'node_modules'))
  await headless(opts)

  expect(await exists(path.join(lockfileDir, 'node_modules/.pnpm/node_modules/diskusage/build/new-file.txt'))).toBeTruthy()

  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created
})

test('installing in a workspace', async () => {
  const workspaceFixture = path.join(__dirname, 'workspace-fixture')

  let { projects } = await readprojectsContext(
    [
      {
        rootDir: path.join(workspaceFixture, 'foo'),
      },
      {
        rootDir: path.join(workspaceFixture, 'bar'),
      },
    ],
    { lockfileDir: workspaceFixture }
  )

  projects = await Promise.all(
    projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) }))
  )

  await headless(await testDefaults({
    lockfileDir: workspaceFixture,
    projects,
  }))

  const projectBar = assertProject(path.join(workspaceFixture, 'bar'))

  await projectBar.has('foo')

  await headless(await testDefaults({
    lockfileDir: workspaceFixture,
    projects: [projects[0]],
  }))

  const rootModules = assertProject(workspaceFixture)
  const lockfile = await rootModules.readCurrentLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/is-negative/1.0.0',
    '/is-positive/1.0.0',
  ])
})

test('installing with no symlinks but with PnP', async () => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))
  await rimraf(path.join(prefix, '.pnp.js'))

  await headless(await testDefaults({
    enablePnp: true,
    lockfileDir: prefix,
    symlink: false,
  }))

  expect([...await fs.readdir(path.join(prefix, 'node_modules'))]).toStrictEqual(['.bin', '.modules.yaml', '.pnpm'])
  expect([...await fs.readdir(path.join(prefix, 'node_modules/.pnpm/rimraf@2.7.1/node_modules'))]).toStrictEqual(['rimraf'])

  const project = assertProject(prefix)
  expect(await project.readCurrentLockfile()).toBeTruthy()
  expect(await project.readModulesManifest()).toBeTruthy()
  expect(await exists(path.join(prefix, '.pnp.js'))).toBeTruthy()
})

test('installing with no modules directory', async () => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))
  await rimraf(path.join(prefix, '.pnp.js'))

  await headless(await testDefaults({
    enableModulesDir: false,
    lockfileDir: prefix,
  }))

  expect(await exists(path.join(prefix, 'node_modules'))).toBeFalsy()
})
