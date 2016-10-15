import tape = require('tape')
import promisifyTape = require('tape-promise')
const test = promisifyTape(tape)
import path = require('path')
import fs = require('fs')
import exists = require('exists-file')
import existsSymlink = require('exists-link')
import prepare from './support/prepare'
import testDefaults from './support/testDefaults'
import {installPkgs, uninstall} from '../src'

test('uninstall package with no dependencies', async function (t) {
  prepare()
  await installPkgs(['is-negative@2.1.0'], testDefaults({ save: true }))
  await uninstall(['is-negative'], testDefaults({ save: true }))

  let stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
  t.ok(!stat, 'is-negative is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'is-negative'))
  t.ok(!stat, 'is-negative is removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {}
  t.deepEqual(dependencies, expectedDeps, 'is-negative has been removed from dependencies')
})

test('uninstall scoped package', async function (t) {
  prepare()
  await installPkgs(['@zkochan/logger@0.1.0'], testDefaults({ save: true }))
  await uninstall(['@zkochan/logger'], testDefaults({ save: true }))

  let stat = await exists(path.join(process.cwd(), 'node_modules', '.store', '@zkochan+logger@0.1.0'))
  t.ok(!stat, '@zkochan/logger is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', '@zkochan/logger'))
  t.ok(!stat, '@zkochan/logger is removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {}
  t.deepEqual(dependencies, expectedDeps, '@zkochan/logger has been removed from dependencies')
})

test('uninstall tarball dependency', async function (t) {
  prepare()
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], testDefaults({ save: true }))
  await uninstall(['is-array'], testDefaults({ save: true }))

  let stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'is-array-1.0.1#a83102a9c117983e6ff4d85311fb322231abe3d6'))
  t.ok(!stat, 'is-array is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'is-array'))
  t.ok(!stat, 'is-array is removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {}
  t.deepEqual(dependencies, expectedDeps, 'is-array has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async function (t) {
  prepare()
  await installPkgs(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], testDefaults({ save: true }))

  let stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
  t.ok(!stat, 'camelcase-keys is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase-keys'))
  t.ok(!stat, 'camelcase-keys is removed from node_modules')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
  t.ok(!stat, 'camelcase is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase'))
  t.ok(!stat, 'camelcase is removed from node_modules')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
  t.ok(!stat, 'map-obj is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'map-obj'))
  t.ok(!stat, 'map-obj is removed from node_modules')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
  t.ok(stat, 'is-negative is not removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'is-negative'))
  t.ok(stat, 'is-negative is not removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {
    'is-negative': '^2.1.0'
  }
  t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')
})

test('uninstall package with its bin files', async function (t) {
  prepare()
  await installPkgs(['sh-hello-world@1.0.0'], testDefaults({ save: true }))
  await uninstall(['sh-hello-world'], testDefaults({ save: true }))

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = await existsSymlink(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')
})

test('keep dependencies used by others', async function (t) {
  prepare()
  await installPkgs(['hastscript@3.0.0', 'camelcase-keys@3.0.0'], testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], testDefaults({ save: true }))

  let stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
  t.ok(!stat, 'camelcase-keys is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase-keys'))
  t.ok(!stat, 'camelcase-keys is removed from node_modules')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
  t.ok(stat, 'camelcase is not removed from store')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
  t.ok(!stat, 'map-obj is removed from store')

  stat = await existsSymlink(path.join(process.cwd(), 'node_modules', 'map-obj'))
  t.ok(!stat, 'map-obj is removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {
    'hastscript': '^3.0.0'
  }
  t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')
})

test('keep dependency used by package', async function (t) {
  prepare()
  await installPkgs(['is-not-positive@1.0.0', 'is-positive@3.1.0'], testDefaults({ save: true }))
  await uninstall(['is-not-positive'], testDefaults({ save: true }))

  let stat = await exists(path.join(process.cwd(), 'node_modules', '.store', 'is-positive@3.1.0'))
  t.ok(stat, 'is-positive is not removed from store')
})
