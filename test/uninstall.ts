import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import exists = require('path-exists')
import existsSymlink = require('exists-link')
import readPkg = require('read-pkg')
import ncpCB = require('ncp')
import {
  prepare,
  testDefaults,
  pathToLocalPkg,
} from './utils'
import {
  installPkgs,
  uninstall,
  link,
  storePrune,
  RootLog,
  StatsLog,
  PackageJsonLog,
} from 'supi'
import promisify = require('util.promisify')
import sinon = require('sinon')

const ncp = promisify(ncpCB.ncp)

test('uninstall package with no dependencies', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], await testDefaults({ save: true }))

  const reporter = sinon.spy()
  await uninstall(['is-negative'], await testDefaults({ save: true, reporter }))

  t.ok(reporter.calledWithMatch(<PackageJsonLog>{
    name: 'pnpm:package-json',
    level: 'debug',
    initial: {
      name: 'project',
      version: '0.0.0',
      dependencies: {
        'is-negative': '^2.1.0',
      },
    },
  }), 'initial package.json logged')
  t.ok(reporter.calledWithMatch(<StatsLog>{
    name: 'pnpm:stats',
    level: 'debug',
    removed: 1,
  }), 'reported info message about removing orphans')
  t.ok(reporter.calledWithMatch(<RootLog>{
    name: 'pnpm:root',
    level: 'info',
    removed: {
      name: 'is-negative',
      version: '2.1.0',
      dependencyType: 'prod',
    },
  }), 'removing root dependency reported')
  t.ok(reporter.calledWithMatch(<PackageJsonLog>{
    name: 'pnpm:package-json',
    level: 'debug',
    updated: {
      name: 'project',
      version: '0.0.0',
    },
  }), 'updated package.json logged')

  // uninstall does not remove packages from store
  // even if they become unreferenced
  await project.storeHas('is-negative', '2.1.0')

  await project.hasNot('is-negative')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, 'is-negative has been removed from dependencies')
})

test('uninstall scoped package', async function (t) {
  const project = prepare(t)
  await installPkgs(['@zkochan/logger@0.1.0'], await testDefaults({ save: true }))
  await uninstall(['@zkochan/logger'], await testDefaults({ save: true }))

  await project.storeHas('@zkochan/logger', '0.1.0')

  await project.hasNot('@zkochan/logger')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, '@zkochan/logger has been removed from dependencies')
})

test('uninstall tarball dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], await testDefaults({ save: true }))
  await uninstall(['is-array'], await testDefaults({ save: true }))

  t.ok(await exists(path.join(await project.getStorePath(), 'registry.npmjs.org', 'is-array', '1.0.1')))

  await project.hasNot('is-array')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, 'is-array has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], await testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], await testDefaults({ save: true }))

  await storePrune(await testDefaults())

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHasNot('camelcase', '3.0.0')
  await project.hasNot('camelcase')

  await project.storeHasNot('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {'is-negative': '^2.1.0'}, 'camelcase-keys has been removed from dependencies')

  const shr = await project.loadShrinkwrap()
  t.deepEqual(shr.dependencies, {
    'is-negative': '2.1.0',
  }, 'camelcase-keys removed from shrinkwrap dependencies')
  t.deepEqual(shr.specifiers, {
    'is-negative': '^2.1.0',
  }, 'camelcase-keys removed from shrinkwrap specifiers')
})

test('uninstall package with its bin files', async function (t) {
  prepare(t)
  await installPkgs(['sh-hello-world@1.0.1'], await testDefaults({ save: true }))
  await uninstall(['sh-hello-world'], await testDefaults({ save: true }))

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = await existsSymlink(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')

  stat = await exists(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')
})

test('relative link is uninstalled', async (t: tape.Test) => {
  const project = prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link([`../${linkedPkgName}`], path.join(process.cwd(), 'node_modules'), await testDefaults())
  await uninstall([linkedPkgName], await testDefaults())

  await project.hasNot(linkedPkgName)
})

test('pendingBuilds gets updated after uninstall', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0', 'sh-hello-world@1.0.1'], await testDefaults({save: true, ignoreScripts: true}))

  const modules1 = await project.loadModules()
  t.doesNotEqual(modules1['pendingBuilds'].length, 0, 'installPkgs should update pendingBuilds')

  await uninstall(['sh-hello-world'], await testDefaults({save: true}))

  const modules2 = await project.loadModules()
  t.doesNotEqual(modules2['pendingBuilds'].length, 0, 'uninstall should not remove all the pendingBuilds')
  t.ok(modules1['pendingBuilds'].length > modules2['pendingBuilds'].length, 'uninstall should update pendingBuilds')
})
