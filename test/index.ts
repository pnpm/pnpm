import assertProject from '@pnpm/assert-project'
import headless from '@pnpm/headless'
import fse = require('fs-extra')
import test = require('tape')
import tempy = require('tempy')
import path = require('path')
import exists = require('path-exists')
import {readWanted} from 'pnpm-shrinkwrap'
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  StageLog,
  StatsLog,
  PackageJsonLog,
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
