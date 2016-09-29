import tape = require('tape')
import promisifyTape = require('tape-promise')
const test = promisifyTape(tape)
import path = require('path')
import fs = require('fs')
import exists, {existsSymlink} from './support/exists'
import prepare from './support/prepare'
import {installPkgs, uninstall} from '../src'

test('uninstall package with no dependencies', async function (t) {
  prepare()
  await installPkgs(['is-negative@2.1.0'], { save: true })
  await uninstall(['is-negative'], { save: true })

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
  await installPkgs(['@zkochan/logger@0.1.0'], { save: true })
  await uninstall(['@zkochan/logger'], { save: true })

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
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], { save: true })
  await uninstall(['is-array'], { save: true })

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
  await installPkgs(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], { save: true })
  await uninstall(['camelcase-keys'], { save: true })

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
  await installPkgs(['sh-hello-world@1.0.0'], { save: true })
  await uninstall(['sh-hello-world'], { save: true })

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = await existsSymlink(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')

  stat = await exists(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')
})

test('keep dependencies used by others', async function (t) {
  prepare()
  await installPkgs(['hastscript@3.0.0', 'camelcase-keys@3.0.0'], { save: true })
  await uninstall(['camelcase-keys'], { save: true })

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
