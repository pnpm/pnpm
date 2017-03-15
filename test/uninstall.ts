import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import fs = require('fs')
import exists = require('path-exists')
import existsSymlink = require('exists-link')
import readPkg = require('read-pkg')
import {prepare, testDefaults} from './utils'
import {installPkgs, uninstall} from '../src'

test('uninstall package with no dependencies', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@2.1.0'], testDefaults({ save: true }))
  await uninstall(['is-negative'], testDefaults({ save: true }))

  await project.storeHasNot('is-negative', '2.1.0')

  await project.hasNot('is-negative')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {}, 'is-negative has been removed from dependencies')
})

test('uninstall scoped package', async function (t) {
  const project = prepare(t)
  await installPkgs(['@zkochan/logger@0.1.0'], testDefaults({ save: true }))
  await uninstall(['@zkochan/logger'], testDefaults({ save: true }))

  await project.storeHasNot('@zkochan/logger', '0.1.0')

  await project.hasNot('@zkochan/logger')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {}, '@zkochan/logger has been removed from dependencies')
})

test('uninstall tarball dependency', async function (t) {
  const project = prepare(t)
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], testDefaults({ save: true }))
  await uninstall(['is-array'], testDefaults({ save: true }))

  await project.storeHasNot('is-array-1.0.1#a83102a9c117983e6ff4d85311fb322231abe3d6')

  await project.hasNot('is-array')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {}, 'is-array has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], testDefaults({ save: true }))

  await project.storeHasNot('camelcase-keys', '2.1.0')
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
    'is-negative@^2.1.0': '2.1.0',
  }, 'camelcase-keys removed from shrinkwrap dependencies')
})

test('uninstall package with its bin files', async function (t) {
  prepare(t)
  await installPkgs(['sh-hello-world@1.0.1'], testDefaults({ save: true }))
  await uninstall(['sh-hello-world'], testDefaults({ save: true }))

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = await existsSymlink(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')

  stat = await exists(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')
})

test('keep dependencies used by others', async function (t) {
  const project = prepare(t)
  await installPkgs(['hastscript@3.0.0', 'camelcase-keys@3.0.0'], testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], testDefaults({ save: true }))

  await project.storeHasNot('camelcase-keys', '2.1.0')
  await project.hasNot('camelcase-keys')

  await project.storeHas('camelcase', '3.0.0')

  await project.storeHasNot('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {'hastscript': '^3.0.0'}, 'camelcase-keys has been removed from dependencies')
})

test('keep dependency used by package', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-not-positive@1.0.0', 'is-positive@3.1.0'], testDefaults({ save: true }))
  await uninstall(['is-not-positive'], testDefaults({ save: true }))

  await project.storeHas('is-positive', '3.1.0')
})
