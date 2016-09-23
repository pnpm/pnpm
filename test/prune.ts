import tape = require('tape')
import promisifyTape = require('tape-promise')
const test = promisifyTape(tape)
import path = require('path')
import prune from '../src/cmd/prune'
import install from '../src/cmd/install'
import prepare from './support/prepare'
import exists, {existsSymlink} from './support/exists'

test('prune removes extraneous packages', async function (t) {
  prepare()

  await install(['is-negative@2.1.0'], {save: true, quiet: true})
  await install(['applyq@0.2.1'], {saveDev: true, quiet: true})
  await install(['fnumber@0.1.0'], {saveOptional: true, quiet: true})
  await install(['is-positive@2.0.0', '@zkochan/logger@0.1.0'], {quiet: true})
  await prune([], {})

  const store = path.join(process.cwd(), 'node_modules', '.store')
  const modules = path.join(process.cwd(), 'node_modules')

  let stat = exists(path.join(store, 'is-positive@2.0.0'))
  t.ok(!stat, 'extraneous package is removed from store')

  stat = existsSymlink(path.join(modules, 'is-positive'))
  t.ok(!stat, 'extraneous package is removed from node_modules')

  stat = exists(path.join(store, '@zkochan+logger@0.1.0'))
  t.ok(!stat, 'scoped extraneous package is removed from store')

  stat = existsSymlink(path.join(modules, '@zkochan/logger'))
  t.ok(!stat, 'scoped extraneous package is removed from node_modules')

  stat = exists(path.join(store, 'is-negative@2.1.0'))
  t.ok(stat, 'dependency package is not removed from store')

  stat = existsSymlink(path.join(modules, 'is-negative'))
  t.ok(stat, 'dependency package is not removed from node_modules')

  stat = exists(path.join(store, 'applyq@0.2.1'))
  t.ok(stat, 'dev dependency package is not removed from store')

  stat = existsSymlink(path.join(modules, 'applyq'))
  t.ok(stat, 'dev dependency package is not removed from node_modules')

  stat = exists(path.join(store, 'fnumber@0.1.0'))
  t.ok(stat, 'optional dependency package is not removed from store')

  stat = existsSymlink(path.join(modules, 'fnumber'))
  t.ok(stat, 'optional dependency package is not removed from node_modules')
})

test('prune removes only the specified extraneous packages', async function (t) {
  prepare()

  await install(['is-positive@2.0.0', 'is-negative@2.1.0'], {quiet: true})
  await prune(['is-positive'], {})

  const store = path.join(process.cwd(), 'node_modules', '.store')
  const modules = path.join(process.cwd(), 'node_modules')

  let stat = exists(path.join(store, 'is-positive@2.0.0'))
  t.ok(!stat, 'extraneous package is removed from store')

  stat = existsSymlink(path.join(modules, 'is-positive'))
  t.ok(!stat, 'extraneous package is removed from node_modules')

  stat = exists(path.join(store, 'is-negative@2.1.0'))
  t.ok(stat, 'dependency package is not removed from store')

  stat = existsSymlink(path.join(modules, 'is-negative'))
  t.ok(stat, 'dependency package is not removed from node_modules')
})

test('prune throws error when trying to removes not an extraneous package', async function (t) {
  prepare()

  await install(['is-positive@2.0.0'], {quiet: true, save: true})

  try {
    await prune(['is-positive'], {})
    t.fail('prune had to fail')
  } catch (err) {
    t.equal(err['code'], 'PRUNE_NOT_EXTR', 'cannot prune non-extraneous package error thrown')
  }
})

test('prune removes dev dependencies in production', async function (t) {
  prepare()

  await install(['is-positive@2.0.0'], {quiet: true, saveDev: true})
  await install(['is-negative@2.1.0'], {quiet: true, save: true})
  await install(['fnumber@0.1.0'], {quiet: true, saveOptional: true})
  await prune([], {production: true})

  const store = path.join(process.cwd(), 'node_modules', '.store')
  const modules = path.join(process.cwd(), 'node_modules')

  let stat = exists(path.join(store, 'is-positive@2.0.0'))
  t.ok(!stat, 'dev dependency package is removed from store')

  stat = existsSymlink(path.join(modules, 'is-positive'))
  t.ok(!stat, 'dev dependency package is removed from node_modules')

  stat = exists(path.join(store, 'is-negative@2.1.0'))
  t.ok(stat, 'dependency package is not removed from store')

  stat = existsSymlink(path.join(modules, 'is-negative'))
  t.ok(stat, 'dependency package is not removed from node_modules')

  stat = exists(path.join(store, 'fnumber@0.1.0'))
  t.ok(stat, 'optional dependency package is not removed from store')

  stat = existsSymlink(path.join(modules, 'fnumber'))
  t.ok(stat, 'optional dependency package is not removed from node_modules')
})
