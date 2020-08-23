/// <reference path="../../../typings/index.d.ts" />
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import prepare, { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { copyFixture } from '@pnpm/test-fixtures'
import './recursive'
import { DEFAULT_OPTS } from './utils'
import execa = require('execa')
import path = require('path')
import exists = require('path-exists')
import sinon = require('sinon')
import test = require('tape')

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('rebuilds dependencies', async (t) => {
  const project = prepareEmpty(t)
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    'pre-and-postinstall-scripts-example',
    'zkochan/install-scripts-example#prepare',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  let modules = await project.readModulesManifest()
  t.deepEqual(modules!.pendingBuilds, [
    '/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/zkochan/install-scripts-example/2de638b8b572cd1e87b74f4540754145fb2c0ebb',
  ])

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: false,
    storeDir,
  }, [])

  modules = await project.readModulesManifest()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  {
    t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-prepare.js'))
    t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))

    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const scripts = project.requireModule('install-scripts-example-for-pnpm/output.json')
    t.equal(scripts[0], 'preinstall')
    t.equal(scripts[1], 'install')
    t.equal(scripts[2], 'postinstall')
    t.equal(scripts[3], 'prepare')
  }
  t.end()
})

test('rebuild does not fail when a linked package is present', async (t) => {
  prepareEmpty(t)
  const storeDir = path.resolve('store')
  await copyFixture('local-pkg', path.resolve('..', 'local-pkg'))

  await execa('node', [
    pnpmBin,
    'add',
    'link:../local-pkg',
    'is-positive',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: false,
    storeDir,
  }, [])

  // see related issue https://github.com/pnpm/pnpm/issues/1155
  t.pass('rebuild did not fail')
  t.end()
})

test('rebuilds specific dependencies', async (t) => {
  const project = prepareEmpty(t)
  const storeDir = path.resolve('store')
  await execa('node', [
    pnpmBin,
    'add',
    '--save-dev',
    'pre-and-postinstall-scripts-example',
    'zkochan/install-scripts-example',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: false,
    storeDir,
  }, ['install-scripts-example-for-pnpm'])

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  t.end()
})

test('rebuild with pending option', async (t) => {
  const project = prepareEmpty(t)
  const storeDir = path.resolve('store')
  await execa('node', [
    pnpmBin,
    'add',
    'pre-and-postinstall-scripts-example',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])
  await execa('node', [
    pnpmBin,
    'add',
    'zkochan/install-scripts-example',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  let modules = await project.readModulesManifest()
  t.deepEqual(modules!.pendingBuilds, [
    '/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/zkochan/install-scripts-example/6d879afcee10ece4d3f0e8c09de2993232f3430a',
  ])

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  await project.hasNot('install-scripts-example-for-pnpm/generated-by-preinstall')
  await project.hasNot('install-scripts-example-for-pnpm/generated-by-postinstall')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: true,
    storeDir,
  }, [])

  modules = await project.readModulesManifest()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
  t.end()
})

test('rebuild dependencies in correct order', async (t) => {
  const project = prepareEmpty(t)
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    'with-postinstall-a',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  let modules = await project.readModulesManifest()
  t.ok(modules)
  t.doesNotEqual(modules!.pendingBuilds.length, 0)

  await project.hasNot('.pnpm/with-postinstall-b@1.0.0/node_modules/with-postinstall-b/output.json')
  await project.hasNot('with-postinstall-a/output.json')

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: false,
    storeDir,
  }, [])

  modules = await project.readModulesManifest()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  t.ok(+project.requireModule('.pnpm/with-postinstall-b@1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0])
  t.end()
})

test('rebuild links bins', async (t) => {
  const project = prepareEmpty(t)
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    'has-generated-bins-as-dep',
    'generated-bins',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  t.notOk(await exists(path.resolve('node_modules/.bin/cmd1')))
  t.notOk(await exists(path.resolve('node_modules/.bin/cmd2')))

  t.ok(await exists(path.resolve('node_modules/has-generated-bins-as-dep/package.json')))
  t.notOk(await exists(path.resolve('node_modules/has-generated-bins-as-dep/node_modules/.bin/cmd1')))
  t.notOk(await exists(path.resolve('node_modules/has-generated-bins-as-dep/node_modules/.bin/cmd2')))

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: true,
    storeDir,
  }, [])

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')
  await project.isExecutable('has-generated-bins-as-dep/node_modules/.bin/cmd1')
  await project.isExecutable('has-generated-bins-as-dep/node_modules/.bin/cmd2')
  t.end()
})

test(`rebuild should not fail on incomplete ${WANTED_LOCKFILE}`, async (t) => {
  prepare(t, {
    dependencies: {
      'pre-and-postinstall-scripts-example': '1.0.0',
    },
    optionalDependencies: {
      'not-compatible-with-any-os': '1.0.0',
    },
  })
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'install',
    '--registry',
    REGISTRY,
    '--store-dir',
    storeDir,
    '--ignore-scripts',
  ])

  const reporter = sinon.spy()

  await rebuild.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    pending: true,
    reporter,
    storeDir,
  }, [])

  t.end()
})
