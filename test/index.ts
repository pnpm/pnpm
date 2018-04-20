import assertProject from '@pnpm/assert-project'
import headless from '@pnpm/headless'
import fse = require('fs-extra')
import test = require('tape')
import tempy = require('tempy')
import path = require('path')
import exists = require('path-exists')
import {readWanted} from 'pnpm-shrinkwrap'
import {read as readModulesYaml} from '@pnpm/modules-yaml'
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  StageLog,
  StatsLog,
  PackageJsonLog,
  ProgressLog,
  RootLog,
} from 'supi'
import testDefaults from './utils/testDefaults'

const fixtures = path.join(__dirname, 'fixtures')

test('installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  const reporter = sinon.spy()

  await headless(await testDefaults({prefix, reporter}))

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
    initial: require(path.join(prefix, 'package.json')),
    level: 'debug',
    name: 'pnpm:package-json',
  } as PackageJsonLog), 'initial package.json logged')
  t.ok(reporter.calledWithMatch({
    added: 15,
    level: 'debug',
    name: 'pnpm:stats',
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    removed: 0,
    level: 'debug',
    name: 'pnpm:stats',
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
  } as ProgressLog), 'logs that package is being resolved')

  t.end()
})

test('installing only prod deps', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({
    prefix,
    production: true,
    development: false,
    optional: false,
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
    prefix,
    production: false,
    development: true,
    optional: false,
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
    prefix,
    production: false,
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
  await headless(await testDefaults({prefix, reporter}))

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'once',
      realName: 'once',
    },
    level: 'info',
    name: 'pnpm:root',
  } as RootLog), 'added to root')
  t.notOk(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'inflight',
      realName: 'inflight',
    },
    level: 'info',
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
    prefix,
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

  await headless(await testDefaults({prefix}))

  const project = assertProject(t, prefix)
  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

  t.deepEqual(require(outputJsonPath), ['install', 'postinstall'])

  await rimraf(outputJsonPath)
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({prefix, ignoreScripts: true}))

  t.notOk(await exists(outputJsonPath))

  const modulesYaml = await readModulesYaml(path.join(prefix, 'node_modules'))
  t.ok(modulesYaml)
  t.deepEqual(modulesYaml!.pendingBuilds, ['localhost+4873/pre-and-postinstall-scripts-example/1.0.0'])

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
    prefix: projectDir,
  }))

  fse.copySync(path.join(simpleDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(simpleDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({
    prefix: projectDir,
    reporter,
  }))

  t.ok(reporter.calledWithMatch({
    removed: 1,
    level: 'debug',
    name: 'pnpm:stats',
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

  await headless(await testDefaults({prefix: projectDir}))

  fse.copySync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobAndRimrafDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({prefix: projectDir, reporter}))

  const project = assertProject(t, projectDir)
  await project.has('rimraf')
  await project.has('glob')

  t.notOk(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/balanced-match/1.0.0',
    status: 'resolving_content',
  } as ProgressLog), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/rimraf/2.6.2',
    status: 'resolving_content',
  } as ProgressLog), 'resolves rimraf')

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

  await headless(await testDefaults({prefix: projectDir}))

  fse.copySync(path.join(hasGlobAndRimrafDir, 'package.json'), destPackageJsonPath)
  fse.copySync(path.join(hasGlobAndRimrafDir, 'shrinkwrap.yaml'), destShrinkwrapYamlPath)

  const reporter = sinon.spy()
  await headless(await testDefaults({prefix: projectDir, reporter, force: true}))

  const project = assertProject(t, projectDir)
  await project.has('rimraf')
  await project.has('glob')

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/balanced-match/1.0.0',
    status: 'resolving_content',
  } as ProgressLog), 'does not resolve already available package')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    pkgId: 'localhost+4873/rimraf/2.6.2',
    status: 'resolving_content',
  } as ProgressLog), 'resolves rimraf')

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
    await headless(await testDefaults({prefix: projectDir}))
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Cannot run headless installation because shrinkwrap.yaml is not up-to-date with package.json')
  }

  t.end()
})

test('installing local dependency', async (t) => {
  const prefix = path.join(fixtures, 'has-local-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({prefix, reporter}))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('tar-pkg'), 'prod dep installed')

  t.end()
})

test('installing local directory dependency', async (t) => {
  const prefix = path.join(fixtures, 'has-local-dir-dep')
  const reporter = sinon.spy()

  await headless(await testDefaults({prefix, reporter}))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('example/package.json'), 'prod dep installed')

  t.end()
})

test('installing using passed in shrinkwrap files', async (t) => {
  const prefix = tempy.directory()
  t.comment(prefix)

  const simplePkgPath = path.join(fixtures, 'simple')
  const wantedShr = await readWanted(simplePkgPath, {ignoreIncompatible: false})
  const pkg = require(path.join(simplePkgPath, 'package.json'))

  await headless(await testDefaults({prefix, wantedShrinkwrap: wantedShr, packageJson: pkg}))

  const project = assertProject(t, prefix)

  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  t.end()
})

test('installation of a dependency that has a resolved peer in subdeps', async (t) => {
  const prefix = path.join(fixtures, 'resolved-peer-deps-in-subdeps')

  await headless(await testDefaults({prefix}))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('pnpm-default-reporter'), 'prod dep installed')

  t.end()
})

test('independent-leaves: installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await rimraf(path.join(prefix, 'node_modules'))
  const reporter = sinon.spy()

  await headless(await testDefaults({prefix, reporter, independentLeaves: true}))

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
    initial: require(path.join(prefix, 'package.json')),
    level: 'debug',
    name: 'pnpm:package-json',
  } as PackageJsonLog), 'initial package.json logged')
  t.ok(reporter.calledWithMatch({
    added: 15,
    level: 'debug',
    name: 'pnpm:stats',
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    removed: 0,
    level: 'debug',
    name: 'pnpm:stats',
  } as StatsLog), 'removed stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    message: 'importing_done',
    name: 'pnpm:stage',
  } as StageLog), 'importing stage done logged')

  t.end()
})
