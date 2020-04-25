///<reference path="../../../typings/index.d.ts" />
import assertProject from '@pnpm/assert-project'
import { WANTED_LOCKFILE } from '@pnpm/constants'
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
import rimraf = require('@zkochan/rimraf')
import fse = require('fs-extra')
import loadJsonFile = require('load-json-file')
import path = require('path')
import exists = require('path-exists')
import readYamlFile from 'read-yaml-file'
import sinon = require('sinon')
import test = require('tape')
import tempy = require('tempy')
import writeJsonFile = require('write-json-file')
import testDefaults from './utils/testDefaults'

const fixtures = path.join(__dirname, 'fixtures')

test('installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  const reporter = sinon.spy()

  await headless(await testDefaults({
    lockfileDir: prefix,
    reporter,
  }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  // test that independent leaves is false by default
  await project.has(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/colors`) // colors is not symlinked from the store

  await project.isExecutable('.bin/rimraf')

  t.ok(await project.readCurrentLockfile())
  t.ok(await project.readModulesManifest())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog), 'updated package.json logged')
  t.ok(reporter.calledWithMatch({
    added: 15,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog), 'removed stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog), 'importing stage done logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    requester: prefix,
    status: 'resolved',
  }), 'logs that package is being resolved')

  t.end()
})

test('installing only prod deps', async (t) => {
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

  const project = assertProject(t, prefix)
  await project.has('is-positive')
  await project.has('rimraf')
  await project.hasNot('is-negative')
  await project.hasNot('colors')

  await project.isExecutable('.bin/rimraf')

  t.end()
})

test('installing only dev deps', async (t) => {
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

  const project = assertProject(t, prefix)
  await project.hasNot('is-positive')
  await project.hasNot('rimraf')
  await project.has('is-negative')
  await project.hasNot('colors')

  t.end()
})

test('installing non-prod deps then all deps', async (t) => {
  const prefix = path.join(fixtures, 'prod-dep-is-dev-subdep')

  await headless(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: prefix,
  }))

  const project = assertProject(t, prefix)
  const inflight = project.requireModule('inflight')
  t.equal(typeof inflight, 'function', 'dev dependency is available')

  await project.hasNot('once')

  {
    const lockfile = await project.readLockfile()
    t.ok(lockfile.packages['/is-positive/1.0.0'].dev === false)
  }

  {
    const currentLockfile = await project.readCurrentLockfile()
    t.notOk(currentLockfile.packages['/is-positive/1.0.0'], `prod dep only not added to current ${WANTED_LOCKFILE}`)
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

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'once',
      realName: 'once',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'added to root')
  t.notOk(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'inflight',
      realName: 'inflight',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'not added to root')

  await project.has('once')

  {
    const currentLockfile = await project.readCurrentLockfile()
    t.ok(currentLockfile.packages['/is-positive/1.0.0'], `prod dep added to current ${WANTED_LOCKFILE}`)
  }

  t.end()
})

test('installing only optional deps', async (t) => {
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

  const project = assertProject(t, prefix)
  await project.hasNot('is-positive')
  await project.hasNot('rimraf')
  await project.hasNot('is-negative')
  await project.has('colors')

  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1958
test('not installing optional deps', async (t) => {
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

  const project = assertProject(t, prefix)
  await project.hasNot('is-positive')
  await project.has('pkg-with-good-optional')

  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1547
test('installing with independent-leaves and hoistPattern=*', async (t) => {
  const lockfileDir = path.join(fixtures, 'with-1-dep')
  await rimraf(path.join(lockfileDir, 'node_modules'))

  const { projects } = await readprojectsContext(
    [
      {
        rootDir: lockfileDir,
      },
    ],
    { lockfileDir },
  )

  await headless(await testDefaults({
    hoistPattern: '*',
    independentLeaves: true,
    lockfileDir: lockfileDir,
    projects: await Promise.all(
      projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) })),
    ),
  }))

  const project = assertProject(t, lockfileDir)
  await project.has('rimraf')
  await project.has('.pnpm/node_modules/glob')
  await project.has('.pnpm/node_modules/path-is-absolute')

  // wrappy is linked directly from the store
  await project.hasNot(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/wrappy/1.0.2`)
  await project.storeHas('wrappy', '1.0.2')

  await project.has(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/rimraf/2.5.1`)

  await project.isExecutable('.bin/rimraf')

  t.end()
})

test('installing with independent-leaves when an optional subdep is skipped', async (t) => {
  const prefix = path.join(fixtures, 'has-incompatible-optional-subdep')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    independentLeaves: true,
    lockfileDir: prefix,
  }))

  const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join(prefix, 'node_modules', '.modules.yaml'))
  t.deepEqual(
    modulesInfo.skipped,
    [
      '/dep-of-optional-pkg/1.0.0',
      '/not-compatible-with-any-os/1.0.0',
    ],
    'optional subdeps skipped',
  )

  const project = assertProject(t, prefix)
  await project.has('pkg-with-optional')

  t.end()
})

test('run pre/postinstall scripts', async (t) => {
  const prefix = path.join(fixtures, 'deps-have-lifecycle-scripts')
  const outputJsonPath = path.join(prefix, 'output.json')
  await rimraf(outputJsonPath)

  await headless(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(t, prefix)
  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

  t.deepEqual(require(outputJsonPath), ['install', 'postinstall'])

  await rimraf(outputJsonPath)
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({ lockfileDir: prefix, ignoreScripts: true }))

  t.notOk(await exists(outputJsonPath))

  const nmPath = path.join(prefix, 'node_modules')
  const modulesYaml = await readModulesYaml(nmPath)
  t.ok(modulesYaml)
  t.deepEqual(
    modulesYaml!.pendingBuilds,
    ['.', '/pre-and-postinstall-scripts-example/1.0.0'],
  )

  t.end()
})

test('orphan packages are removed', async (t) => {
  const projectDir = tempy.directory()
  t.comment(projectDir)

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const simpleWithMoreDepsDir = path.join(fixtures, 'simple-with-more-deps')
  const simpleDir = path.join(fixtures, 'simple')
  fse.copySync(path.join(simpleWithMoreDepsDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(simpleWithMoreDepsDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headless(await testDefaults({
    lockfileDir: projectDir,
  }))

  fse.copySync(path.join(simpleDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(simpleDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({
    lockfileDir: projectDir,
    reporter,
  }))

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: projectDir,
    removed: 1,
  } as StatsLog), 'removed stat')

  const project = assertProject(t, projectDir)
  await project.hasNot('resolve-from')
  await project.has('rimraf')
  await project.has('is-negative')
  await project.has('colors')

  t.end()
})

test('available packages are used when node_modules is not clean', async (t) => {
  const projectDir = tempy.directory()
  t.comment(projectDir)

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const hasGlobDir = path.join(fixtures, 'has-glob')
  const hasGlobAndRimrafDir = path.join(fixtures, 'has-glob-and-rimraf')
  fse.copySync(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headless(await testDefaults({ lockfileDir: projectDir }))

  fse.copySync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobAndRimrafDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({ lockfileDir: projectDir, reporter }))

  const project = assertProject(t, projectDir)
  await project.has('rimraf')
  await project.has('glob')

  t.notOk(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/balanced-match/1.0.0`,
    requester: projectDir,
    status: 'resolved',
  }), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/rimraf/2.6.2`,
    requester: projectDir,
    status: 'resolved',
  }), 'resolves rimraf')

  t.end()
})

test('available packages are relinked during forced install', async (t) => {
  const projectDir = tempy.directory()
  t.comment(projectDir)

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destLockfileYamlPath = path.join(projectDir, WANTED_LOCKFILE)

  const hasGlobDir = path.join(fixtures, 'has-glob')
  const hasGlobAndRimrafDir = path.join(fixtures, 'has-glob-and-rimraf')
  fse.copySync(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobDir, WANTED_LOCKFILE), destLockfileYamlPath)

  await headless(await testDefaults({ lockfileDir: projectDir }))

  fse.copySync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobAndRimrafDir, WANTED_LOCKFILE), destLockfileYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({ lockfileDir: projectDir, reporter, force: true }))

  const project = assertProject(t, projectDir)
  await project.has('rimraf')
  await project.has('glob')

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/balanced-match/1.0.0`,
    requester: projectDir,
    status: 'resolved',
  }), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/rimraf/2.6.2`,
    requester: projectDir,
    status: 'resolved',
  }), 'resolves rimraf')

  t.end()
})

test(`fail when ${WANTED_LOCKFILE} is not up-to-date with package.json`, async (t) => {
  const projectDir = tempy.directory()
  t.comment(projectDir)

  const simpleDir = path.join(fixtures, 'simple')
  fse.copySync(path.join(simpleDir, 'package.json'), path.join(projectDir, 'package.json'))

  const simpleWithMoreDepsDir = path.join(fixtures, 'simple-with-more-deps')
  fse.copySync(path.join(simpleWithMoreDepsDir, WANTED_LOCKFILE), path.join(projectDir, WANTED_LOCKFILE))

  try {
    await headless(await testDefaults({ lockfileDir: projectDir }))
    t.fail()
  } catch (err) {
    t.equal(err.message, `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with package.json`)
  }

  t.end()
})

test('installing local dependency', async (t) => {
  const prefix = path.join(fixtures, 'has-local-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('tar-pkg'), 'prod dep installed')

  t.end()
})

test('installing local directory dependency', async (t) => {
  const prefix = path.join(fixtures, 'has-local-dir-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('example/package.json'), 'prod dep installed')

  t.end()
})

test('installing using passed in lockfile files', async (t) => {
  const prefix = tempy.directory()
  t.comment(prefix)

  const simplePkgPath = path.join(fixtures, 'simple')
  fse.copySync(path.join(simplePkgPath, 'package.json'), path.join(prefix, 'package.json'))
  fse.copySync(path.join(simplePkgPath, WANTED_LOCKFILE), path.join(prefix, WANTED_LOCKFILE))

  const wantedLockfile = await readWantedLockfile(simplePkgPath, { ignoreIncompatible: false })

  await headless(await testDefaults({
    lockfileDir: prefix,
    wantedLockfile,
  }))

  const project = assertProject(t, prefix)

  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  t.end()
})

test('installation of a dependency that has a resolved peer in subdeps', async (t) => {
  const prefix = path.join(fixtures, 'resolved-peer-deps-in-subdeps')

  await headless(await testDefaults({ lockfileDir: prefix }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('pnpm-default-reporter'), 'prod dep installed')

  t.end()
})

test('independent-leaves: installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter, independentLeaves: true }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')
  await project.has(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/rimraf`) // rimraf is not symlinked from the store
  await project.hasNot(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/colors`) // colors is symlinked from the store

  await project.isExecutable('.bin/rimraf')

  t.ok(await project.readCurrentLockfile())
  t.ok(await project.readModulesManifest())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog), 'updated package.json logged')
  t.ok(reporter.calledWithMatch({
    added: 15,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog), 'removed stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog), 'importing stage done logged')

  t.end()
})

test('installing with hoistPattern=*', async (t) => {
  const prefix = path.join(fixtures, 'simple-shamefully-flatten')
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter, hoistPattern: '*' }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('.pnpm/node_modules/glob'), 'prod subdep hoisted')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  // test that independent leaves is false by default
  await project.has(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/colors`) // colors is not symlinked from the store

  await project.isExecutable('.bin/rimraf')
  await project.isExecutable('.pnpm/node_modules/.bin/hello-world-js-bin')

  t.ok(await project.readCurrentLockfile())
  t.ok(await project.readModulesManifest())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog), 'updated package.json logged')
  t.ok(reporter.calledWithMatch({
    added: 17,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog), 'removed stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog), 'importing stage done logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    requester: prefix,
    status: 'resolved',
  }), 'logs that package is being resolved')

  const modules = await project.readModulesManifest()

  t.deepEqual(modules!.hoistedAliases[`localhost+${REGISTRY_MOCK_PORT}/balanced-match/1.0.0`], ['balanced-match'], 'hoisted field populated in .modules.yaml')

  t.end()
})

test('installing with hoistPattern=* and shamefullyHoist=true', async (t) => {
  const prefix = path.join(fixtures, 'simple-shamefully-flatten')
  await rimraf(path.join(prefix, 'node_modules'))
  const reporter = sinon.spy()

  await headless(await testDefaults({ lockfileDir: prefix, reporter, hoistPattern: '*', shamefullyHoist: true }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('glob'), 'prod subdep hoisted')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  // test that independent leaves is false by default
  await project.has(`.pnpm/localhost+${REGISTRY_MOCK_PORT}/colors`) // colors is not symlinked from the store

  await project.isExecutable('.bin/rimraf')
  await project.isExecutable('.bin/hello-world-js-bin')

  t.ok(await project.readCurrentLockfile())
  t.ok(await project.readModulesManifest())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageManifestLog), 'updated package.json logged')
  t.ok(reporter.calledWithMatch({
    added: 17,
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix,
    removed: 0,
  } as StatsLog), 'removed stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix,
    stage: 'importing_done',
  } as StageLog), 'importing stage done logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    requester: prefix,
    status: 'resolved',
  }), 'logs that package is being resolved')

  const modules = await project.readModulesManifest()

  t.deepEqual(modules!.hoistedAliases[`localhost+${REGISTRY_MOCK_PORT}/balanced-match/1.0.0`], ['balanced-match'], 'hoisted field populated in .modules.yaml')

  t.end()
})

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

test.skip('using side effects cache', async (t) => {
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

  const cacheIntegrityPath = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/integrity.json`)
  const cacheIntegrity = await loadJsonFile(cacheIntegrityPath)
  t.ok(cacheIntegrity['build/Makefile'])
  delete cacheIntegrity['build/Makefile']

  t.ok(cacheIntegrity['build/binding.Makefile'])
  await writeJsonFile(cacheIntegrityPath, cacheIntegrity)

  await rimraf(path.join(prefix, 'node_modules'))
  await headless(opts)

  t.notOk(await exists(path.join(prefix, 'node_modules/diskusage/build/Makefile')), 'side effects cache correctly used')
  t.ok(await exists(path.join(prefix, 'node_modules/diskusage/build/binding.Makefile')), 'side effects cache correctly used')

  t.end()
})

test.skip('using side effects cache and hoistPattern=*', async (t) => {
  const lockfileDir = path.join(fixtures, 'side-effects-of-subdep')

  const { projects } = await readprojectsContext(
    [
      {
        rootDir: lockfileDir,
      },
    ],
    { lockfileDir },
  )

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    hoistPattern: '*',
    lockfileDir,
    projects: await Promise.all(
      projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) })),
    ),
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headless(opts)

  const project = assertProject(t, lockfileDir)
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  fse.writeFileSync(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf(path.join(lockfileDir, 'node_modules'))
  await headless(opts)

  t.ok(await exists(path.join(lockfileDir, 'node_modules/.pnpm/node_modules/diskusage/build/new-file.txt')), 'side effects cache correctly used')

  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  t.end()
})

test('installing in a workspace', async (t) => {
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
    { lockfileDir: workspaceFixture },
  )

  projects = await Promise.all(
    projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) })),
  )

  await headless(await testDefaults({
    lockfileDir: workspaceFixture,
    projects,
  }))

  const projectBar = assertProject(t, path.join(workspaceFixture, 'bar'))

  await projectBar.has('foo')

  await headless(await testDefaults({
    lockfileDir: workspaceFixture,
    projects: [projects[0]],
  }))

  const rootNodeModules = assertProject(t, workspaceFixture)
  const lockfile = await rootNodeModules.readCurrentLockfile()
  t.deepEqual(Object.keys(lockfile.packages), [
    '/is-negative/1.0.0',
    '/is-positive/1.0.0',
  ], `packages of project that was not selected by last installation are not removed from current ${WANTED_LOCKFILE}`)

  t.end()
})

test('independent-leaves: installing in a workspace', async (t) => {
  const workspaceFixture = path.join(__dirname, 'workspace-fixture2')

  const { projects } = await readprojectsContext(
    [
      {
        rootDir: path.join(workspaceFixture, 'foo'),
      },
      {
        rootDir: path.join(workspaceFixture, 'bar'),
      },
    ],
    { lockfileDir: workspaceFixture },
  )

  await headless(await testDefaults({
    independentLeaves: true,
    lockfileDir: workspaceFixture,
    projects: await Promise.all(
      projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) })),
    ),
  }))

  const projectBar = assertProject(t, path.join(workspaceFixture, 'bar'))

  await projectBar.has('foo')
  t.ok(await exists(path.join(workspaceFixture, `node_modules/.pnpm/localhost+${REGISTRY_MOCK_PORT}/express/4.16.4/node_modules/array-flatten`)), 'independent package linked')

  t.end()
})
