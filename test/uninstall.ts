import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import fs = require('fs')
import exists = require('path-exists')
import existsSymlink = require('exists-link')
import readPkg = require('read-pkg')
import ncpCB = require('ncp')
import R = require('ramda')
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
} from 'supi'
import thenify = require('thenify')
import sinon = require('sinon')
import {RootLog} from 'pnpm-logger'

const ncp = thenify(ncpCB.ncp)

test('uninstall package with no dependencies', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()
  await installPkgs(['is-negative@2.1.0'], testDefaults({ save: true }))
  await uninstall(['is-negative'], testDefaults({ save: true, reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Removing 1 orphan packages from node_modules',
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

  // uninstall does not remove packages from store
  // even if they become unreferenced
  await project.storeHas('is-negative', '2.1.0')

  await project.hasNot('is-negative')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, 'is-negative has been removed from dependencies')
})

test('uninstall scoped package', async function (t) {
  const project = prepare(t)
  await installPkgs(['@zkochan/logger@0.1.0'], testDefaults({ save: true }))
  await uninstall(['@zkochan/logger'], testDefaults({ save: true }))

  await project.storeHas('@zkochan/logger', '0.1.0')

  await project.hasNot('@zkochan/logger')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, '@zkochan/logger has been removed from dependencies')
})

test('uninstall tarball dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], testDefaults({ save: true }))
  await uninstall(['is-array'], testDefaults({ save: true }))

  t.ok(await exists(path.join(await project.getStorePath(), 'registry.npmjs.org', 'is-array', '1.0.1')))

  await project.hasNot('is-array')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, 'is-array has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], testDefaults({ save: true }))

  await storePrune(testDefaults())

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
  await installPkgs(['sh-hello-world@1.0.1'], testDefaults({ save: true }))
  await uninstall(['sh-hello-world'], testDefaults({ save: true }))

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = await existsSymlink(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')

  stat = await exists(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')
})

test('relative link is uninstalled', async function (t) {
  const project = prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link(`../${linkedPkgName}`, process.cwd(), testDefaults())
  await uninstall([linkedPkgName], testDefaults())

  await project.hasNot(linkedPkgName)
})
