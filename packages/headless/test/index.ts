///<reference path="../typings/index.d.ts" />
import assertProject from '@pnpm/assert-project'
import {
  PackageJsonLog,
  RootLog,
  StageLog,
  StatsLog,
} from '@pnpm/core-loggers'
import headless from '@pnpm/headless'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import readManifests from '@pnpm/read-manifests'
import { readWanted } from '@pnpm/shrinkwrap-file'
import fse = require('fs-extra')
import path = require('path')
import exists = require('path-exists')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import test = require('tape')
import tempy = require('tempy')
import testDefaults from './utils/testDefaults'

const fixtures = path.join(__dirname, 'fixtures')

test('installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  const reporter = sinon.spy()

  await headless(await testDefaults({
    reporter,
    shrinkwrapDirectory: prefix,
  }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  // test that independent leaves is false by default
  t.ok(project.has('.localhost+4873/colors'), 'colors is not symlinked from the store')

  await project.isExecutable('.bin/rimraf')

  t.ok(await project.loadCurrentShrinkwrap())
  t.ok(await project.loadModules())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-json',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageJsonLog), 'updated package.json logged')
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
    packageId: 'localhost+4873/is-negative/2.1.0',
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
    shrinkwrapDirectory: prefix,
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
    shrinkwrapDirectory: prefix,
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
    shrinkwrapDirectory: prefix,
  }))

  const project = assertProject(t, prefix)
  const inflight = project.requireModule('inflight')
  t.equal(typeof inflight, 'function', 'dev dependency is available')

  await project.hasNot('once')

  {
    const shr = await project.loadShrinkwrap()
    t.ok(shr.packages['/is-positive/1.0.0'].dev === false)
  }

  {
    const currentShrinkwrap = await project.loadCurrentShrinkwrap()
    t.notOk(currentShrinkwrap.packages['/is-positive/1.0.0'], 'prod dep only not added to current shrinkwrap.yaml')
  }

  const reporter = sinon.spy()

  // Repeat normal installation adds missing deps to node_modules
  await headless(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    reporter,
    shrinkwrapDirectory: prefix,
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
    const currentShrinkwrap = await project.loadCurrentShrinkwrap()
    t.ok(currentShrinkwrap.packages['/is-positive/1.0.0'], 'prod dep added to current shrinkwrap.yaml')
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
    optional: true,
    production: false,
    shrinkwrapDirectory: prefix,
  }))

  const project = assertProject(t, prefix)
  await project.hasNot('is-positive')
  await project.hasNot('rimraf')
  await project.hasNot('is-negative')
  await project.has('colors')

  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1547
test('installing with independent-leaves and shamefully-flatten', async (t) => {
  const prefix = path.join(fixtures, 'with-1-dep')
  await rimraf(path.join(prefix, 'node_modules'))

  const { importers } = await readManifests(
    [
      {
        prefix,
      },
    ],
    prefix,
    {
      shamefullyFlatten: true,
    },
  )

  await headless(await testDefaults({
    importers,
    independentLeaves: true,
    shrinkwrapDirectory: prefix,
  }))

  const project = assertProject(t, prefix)
  await project.has('rimraf')
  await project.has('glob')
  await project.has('path-is-absolute')

  // wrappy is linked directly from the store
  await project.hasNot('.localhost+4873/wrappy/1.0.2')
  await project.storeHas('wrappy', '1.0.2')

  await project.has('.localhost+4873/rimraf/2.5.1')

  await project.isExecutable('.bin/rimraf')

  t.end()
})

test('run pre/postinstall scripts', async (t) => {
  const prefix = path.join(fixtures, 'deps-have-lifecycle-scripts')
  const outputJsonPath = path.join(prefix, 'output.json')
  await rimraf(outputJsonPath)

  await headless(await testDefaults({ shrinkwrapDirectory: prefix }))

  const project = assertProject(t, prefix)
  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

  t.deepEqual(require(outputJsonPath), ['install', 'postinstall'])

  await rimraf(outputJsonPath)
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({ shrinkwrapDirectory: prefix, ignoreScripts: true }))

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
  const destShrinkwrapYamlPath = path.join(projectDir, 'shrinkwrap.yaml')

  const simpleWithMoreDepsDir = path.join(fixtures, 'simple-with-more-deps')
  const simpleDir = path.join(fixtures, 'simple')
  fse.copySync(path.join(simpleWithMoreDepsDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(simpleWithMoreDepsDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  await headless(await testDefaults({
    shrinkwrapDirectory: projectDir,
  }))

  fse.copySync(path.join(simpleDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(simpleDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({
    reporter,
    shrinkwrapDirectory: projectDir,
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
  const destShrinkwrapYamlPath = path.join(projectDir, 'shrinkwrap.yaml')

  const hasGlobDir = path.join(fixtures, 'has-glob')
  const hasGlobAndRimrafDir = path.join(fixtures, 'has-glob-and-rimraf')
  fse.copySync(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  await headless(await testDefaults({ shrinkwrapDirectory: projectDir }))

  fse.copySync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobAndRimrafDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({ shrinkwrapDirectory: projectDir, reporter }))

  const project = assertProject(t, projectDir)
  await project.has('rimraf')
  await project.has('glob')

  t.notOk(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'localhost+4873/balanced-match/1.0.0',
    requester: projectDir,
    status: 'resolved',
  }), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'localhost+4873/rimraf/2.6.2',
    requester: projectDir,
    status: 'resolved',
  }), 'resolves rimraf')

  t.end()
})

test('available packages are relinked during forced install', async (t) => {
  const projectDir = tempy.directory()
  t.comment(projectDir)

  const destPackageJsonPath = path.join(projectDir, 'package.json')
  const destShrinkwrapYamlPath = path.join(projectDir, 'shrinkwrap.yaml')

  const hasGlobDir = path.join(fixtures, 'has-glob')
  const hasGlobAndRimrafDir = path.join(fixtures, 'has-glob-and-rimraf')
  fse.copySync(path.join(hasGlobDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  await headless(await testDefaults({ shrinkwrapDirectory: projectDir }))

  fse.copySync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobAndRimrafDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({ shrinkwrapDirectory: projectDir, reporter, force: true }))

  const project = assertProject(t, projectDir)
  await project.has('rimraf')
  await project.has('glob')

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'localhost+4873/balanced-match/1.0.0',
    requester: projectDir,
    status: 'resolved',
  }), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: 'localhost+4873/rimraf/2.6.2',
    requester: projectDir,
    status: 'resolved',
  }), 'resolves rimraf')

  t.end()
})

test('fail when shrinkwrap.yaml is not up-to-date with package.json', async (t) => {
  const projectDir = tempy.directory()
  t.comment(projectDir)

  const simpleDir = path.join(fixtures, 'simple')
  fse.copySync(path.join(simpleDir, 'package.json'), path.join(projectDir, 'package.json'))

  const simpleWithMoreDepsDir = path.join(fixtures, 'simple-with-more-deps')
  fse.copySync(path.join(simpleWithMoreDepsDir, 'shrinkwrap.yaml'), path.join(projectDir, 'shrinkwrap.yaml'))

  try {
    await headless(await testDefaults({ shrinkwrapDirectory: projectDir }))
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Cannot install with "frozen-shrinkwrap" because shrinkwrap.yaml is not up-to-date with package.json')
  }

  t.end()
})

test('installing local dependency', async (t) => {
  const prefix = path.join(fixtures, 'has-local-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({ shrinkwrapDirectory: prefix, reporter }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('tar-pkg'), 'prod dep installed')

  t.end()
})

test('installing local directory dependency', async (t) => {
  const prefix = path.join(fixtures, 'has-local-dir-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({ shrinkwrapDirectory: prefix, reporter }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('example/package.json'), 'prod dep installed')

  t.end()
})

test('installing using passed in shrinkwrap files', async (t) => {
  const prefix = tempy.directory()
  t.comment(prefix)

  const simplePkgPath = path.join(fixtures, 'simple')
  fse.copySync(path.join(simplePkgPath, 'package.json'), path.join(prefix, 'package.json'))
  fse.copySync(path.join(simplePkgPath, 'shrinkwrap.yaml'), path.join(prefix, 'shrinkwrap.yaml'))

  const wantedShr = await readWanted(simplePkgPath, { ignoreIncompatible: false })

  await headless(await testDefaults({
    shrinkwrapDirectory: prefix,
    wantedShrinkwrap: wantedShr,
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

  await headless(await testDefaults({ shrinkwrapDirectory: prefix }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('pnpm-default-reporter'), 'prod dep installed')

  t.end()
})

test('independent-leaves: installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))
  const reporter = sinon.spy()

  await headless(await testDefaults({ shrinkwrapDirectory: prefix, reporter, independentLeaves: true }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')
  t.ok(project.has('.localhost+4873/rimraf'), 'rimraf is not symlinked from the store')
  t.ok(project.hasNot('.localhost+4873/colors'), 'colors is symlinked from the store')

  await project.isExecutable('.bin/rimraf')

  t.ok(await project.loadCurrentShrinkwrap())
  t.ok(await project.loadModules())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-json',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageJsonLog), 'updated package.json logged')
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

test('installing with shamefullyFlatten = true', async (t) => {
  const prefix = path.join(fixtures, 'simple-shamefully-flatten')
  const reporter = sinon.spy()

  await headless(await testDefaults({ shrinkwrapDirectory: prefix, reporter, shamefullyFlatten: true }))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('glob'), 'prod subdep hoisted')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  // test that independent leaves is false by default
  t.ok(project.has('.localhost+4873/colors'), 'colors is not symlinked from the store')

  await project.isExecutable('.bin/rimraf')

  t.ok(await project.loadCurrentShrinkwrap())
  t.ok(await project.loadModules())

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-json',
    updated: require(path.join(prefix, 'package.json')),
  } as PackageJsonLog), 'updated package.json logged')
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
    packageId: 'localhost+4873/is-negative/2.1.0',
    requester: prefix,
    status: 'resolved',
  }), 'logs that package is being resolved')

  const modules = await project.loadModules()

  t.deepEqual(modules!.importers['.'].hoistedAliases['localhost+4873/balanced-match/1.0.0'], ['balanced-match'], 'hoisted field populated in .modules.yaml')

  t.end()
})

test('using side effects cache', async (t) => {
  const prefix = path.join(fixtures, 'side-effects')

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    shrinkwrapDirectory: prefix,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headless(opts)

  const cacheBuildDir = path.join(opts.store, 'localhost+4873', 'runas', '3.1.1', 'side_effects', `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`, 'package', 'build')
  fse.writeFileSync(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf(path.join(prefix, 'node_modules'))
  await headless(opts)

  t.ok(await exists(path.join(prefix, 'node_modules', 'runas', 'build', 'new-file.txt')), 'side effects cache correctly used')

  t.end()
})

test('using side effects cache and shamefully-flatten', async (t) => {
  const prefix = path.join(fixtures, 'side-effects-of-subdep')

  const { importers } = await readManifests(
    [
      {
        prefix,
      },
    ],
    prefix,
    {
      shamefullyFlatten: true,
    },
  )

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    importers,
    shrinkwrapDirectory: prefix,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await headless(opts)

  const project = assertProject(t, prefix)
  await project.has('es5-ext') // verifying that a flat node_modules was created

  const cacheBuildDir = path.join(opts.store, 'localhost+4873', 'runas', '3.1.1', 'side_effects', `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`, 'package', 'build')
  fse.writeFileSync(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf(path.join(prefix, 'node_modules'))
  await headless(opts)

  t.ok(await exists(path.join(prefix, 'node_modules', 'runas', 'build', 'new-file.txt')), 'side effects cache correctly used')

  await project.has('es5-ext') // verifying that a flat node_modules was created

  t.end()
})

test('installing in a workspace', async (t) => {
  const workspaceFixture = path.join(__dirname, 'workspace-fixture')

  const manifests = await readManifests(
    [
      {
        prefix: path.join(workspaceFixture, 'foo'),
      },
      {
        prefix: path.join(workspaceFixture, 'bar'),
      },
    ],
    workspaceFixture,
    {
      shamefullyFlatten: false,
    },
  )

  await headless(await testDefaults({
    importers: manifests.importers,
    shrinkwrapDirectory: workspaceFixture,
  }))

  const projectBar = assertProject(t, path.join(workspaceFixture, 'bar'))

  await projectBar.has('foo')

  await headless(await testDefaults({
    importers: [manifests.importers[0]],
    shrinkwrapDirectory: workspaceFixture,
  }))

  const rootNodeModules = assertProject(t, workspaceFixture)
  const shr = await rootNodeModules.loadCurrentShrinkwrap()
  t.deepEqual(Object.keys(shr.packages), [
    '/is-negative/1.0.0',
    '/is-positive/1.0.0',
  ], 'packages of importer that was not selected by last installation are not removed from current shrinkwrap.yaml')

  t.end()
})

test('independent-leaves: installing in a workspace', async (t) => {
  const workspaceFixture = path.join(__dirname, 'workspace-fixture2')

  const { importers } = await readManifests(
    [
      {
        prefix: path.join(workspaceFixture, 'foo'),
      },
      {
        prefix: path.join(workspaceFixture, 'bar'),
      },
    ],
    workspaceFixture,
    {
      shamefullyFlatten: false,
    },
  )

  await headless(await testDefaults({
    importers,
    independentLeaves: true,
    shrinkwrapDirectory: workspaceFixture,
  }))

  const projectBar = assertProject(t, path.join(workspaceFixture, 'bar'))

  await projectBar.has('foo')
  t.ok(await exists(path.join(workspaceFixture, 'node_modules', '.localhost+4873', 'express', '4.16.4', 'node_modules', 'array-flatten')), 'independent package linked')

  t.end()
})
