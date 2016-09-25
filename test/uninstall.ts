import tape = require('tape')
import promisifyTape = require('tape-promise')
const test = promisifyTape(tape)
import path = require('path')
import fs = require('fs')
import exists from './support/exists'
import prepare from './support/prepare'
import install from '../src/cmd/install'
import uninstall from '../src/cmd/uninstall'

function existsSymlink (path: string) {
  try {
    return fs.lstatSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}

test('uninstall package with no dependencies', async function (t) {
  prepare()
  await install(['is-negative@2.1.0'], { quiet: true, save: true })
  await uninstall(['is-negative'], { save: true })

  let stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
  t.ok(!stat, 'is-negative is removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'is-negative'))
  t.ok(!stat, 'is-negative is removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {}
  t.deepEqual(dependencies, expectedDeps, 'is-negative has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async function (t) {
  prepare()
  await install(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], { quiet: true, save: true })
  await uninstall(['camelcase-keys'], { save: true })

  let stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
  t.ok(!stat, 'camelcase-keys is removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase-keys'))
  t.ok(!stat, 'camelcase-keys is removed from node_modules')

  stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
  t.ok(!stat, 'camelcase is removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase'))
  t.ok(!stat, 'camelcase is removed from node_modules')

  stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
  t.ok(!stat, 'map-obj is removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'map-obj'))
  t.ok(!stat, 'map-obj is removed from node_modules')

  stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
  t.ok(stat, 'is-negative is not removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'is-negative'))
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
  await install(['sh-hello-world@1.0.0'], { quiet: true, save: true })
  await uninstall(['sh-hello-world'], { save: true })

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = existsSymlink(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')

  stat = exists(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
  t.ok(!stat, 'sh-hello-world is removed from .bin')
})

test('keep dependencies used by others', async function (t) {
  prepare()
  await install(['hastscript@3.0.0', 'camelcase-keys@3.0.0'], { quiet: true, save: true })
  await uninstall(['camelcase-keys'], { save: true })

  let stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
  t.ok(!stat, 'camelcase-keys is removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase-keys'))
  t.ok(!stat, 'camelcase-keys is removed from node_modules')

  stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
  t.ok(stat, 'camelcase is not removed from store')

  stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
  t.ok(!stat, 'map-obj is removed from store')

  stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'map-obj'))
  t.ok(!stat, 'map-obj is removed from node_modules')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {
    'hastscript': '^3.0.0'
  }
  t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')
})
