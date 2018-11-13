///<reference path="../typings/index.d.ts" />
import assertProject from '@pnpm/assert-project'
import {
  StageLog,
  StatsLog,
  RootLog,
} from '@pnpm/core-loggers'
import headless from '@pnpm/headless'
import fse = require('fs-extra')
import test = require('tape')
import tempy = require('tempy')
import path = require('path')
import exists = require('path-exists')
import { readWanted } from 'pnpm-shrinkwrap'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import readManifests from '@pnpm/read-manifests'
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import testDefaults from './utils/testDefaults'

const fixtures = path.join(__dirname, 'fixtures')

test('installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  const reporter = sinon.spy()

  await headless(await testDefaults({
    shrinkwrapDirectory: prefix,
    reporter,
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
    message: 'importing_done',
    name: 'pnpm:stage',
  } as StageLog), 'importing stage done logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/is-negative/2.1.0',
    status: 'resolving_content',
  }), 'logs that package is being resolved')

  t.end()
})

test('installing only prod deps', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    shrinkwrapDirectory: prefix,
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
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
    shrinkwrapDirectory: prefix,
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
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
    shrinkwrapDirectory: prefix,
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: true,
    },
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
    shrinkwrapDirectory: prefix,
    reporter,
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
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
    shrinkwrapDirectory: prefix,
    include: {
      dependencies: false,
      devDependencies: false,
      optionalDependencies: true,
    },
    production: false,
    development: false,
    optional: true,
  }))

  const project = assertProject(t, prefix)
  await project.hasNot('is-positive')
  await project.hasNot('rimraf')
  await project.hasNot('is-negative')
  await project.has('colors')

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
    ['.', 'localhost+4873/pre-and-postinstall-scripts-example/1.0.0'],
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
    shrinkwrapDirectory: projectDir,
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
    pkgId: 'localhost+4873/balanced-match/1.0.0',
    status: 'resolving_content',
  }), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/rimraf/2.6.2',
    status: 'resolving_content',
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
    pkgId: 'localhost+4873/balanced-match/1.0.0',
    status: 'resolving_content',
  }), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/rimraf/2.6.2',
    status: 'resolving_content',
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
    message: 'importing_done',
    name: 'pnpm:stage',
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
    message: 'importing_done',
    name: 'pnpm:stage',
  } as StageLog), 'importing stage done logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/is-negative/2.1.0',
    status: 'resolving_content',
  }), 'logs that package is being resolved')

  const modules = await project.loadModules()

  t.deepEqual(modules!.importers['.'].hoistedAliases['localhost+4873/balanced-match/1.0.0'], ['balanced-match'], 'hoisted field populated in .modules.yaml')

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

  t.end()
})
