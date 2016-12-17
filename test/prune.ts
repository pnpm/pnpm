import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import {installPkgs, prune, prunePkgs} from '../src'
import prepare from './support/prepare'
import exists = require('exists-file')
import existsSymlink = require('exists-link')
import testDefaults from './support/testDefaults'

test('prune removes extraneous packages', async function (t) {
  prepare(t)

  await installPkgs(['is-negative@2.1.0'], testDefaults({save: true}))
  await installPkgs(['applyq@0.2.1'], testDefaults({saveDev: true}))
  await installPkgs(['fnumber@0.1.0'], testDefaults({saveOptional: true}))
  await installPkgs(['is-positive@2.0.0', '@zkochan/logger@0.1.0'], testDefaults())
  await prune(testDefaults())

  const store = path.join(process.cwd(), 'node_modules/.store')
  const modules = path.join(process.cwd(), 'node_modules')

  let stat = await exists(path.join(store, 'is-positive@2.0.0'))
  t.ok(!stat, 'extraneous package is removed from store')

  stat = await existsSymlink(path.join(modules, 'is-positive'))
  t.ok(!stat, 'extraneous package is removed from node_modules')

  stat = await exists(path.join(store, '@zkochan+logger@0.1.0'))
  t.ok(!stat, 'scoped extraneous package is removed from store')

  stat = await existsSymlink(path.join(modules, '@zkochan/logger'))
  t.ok(!stat, 'scoped extraneous package is removed from node_modules')

  stat = await exists(path.join(store, 'is-negative@2.1.0'))
  t.ok(stat, 'dependency package is not removed from store')

  stat = await existsSymlink(path.join(modules, 'is-negative'))
  t.ok(stat, 'dependency package is not removed from node_modules')

  stat = await exists(path.join(store, 'applyq@0.2.1'))
  t.ok(stat, 'dev dependency package is not removed from store')

  stat = await existsSymlink(path.join(modules, 'applyq'))
  t.ok(stat, 'dev dependency package is not removed from node_modules')

  stat = await exists(path.join(store, 'fnumber@0.1.0'))
  t.ok(stat, 'optional dependency package is not removed from store')

  stat = await existsSymlink(path.join(modules, 'fnumber'))
  t.ok(stat, 'optional dependency package is not removed from node_modules')
})

test('prune removes only the specified extraneous packages', async function (t) {
  prepare(t)

  await installPkgs(['is-positive@2.0.0', 'is-negative@2.1.0'], testDefaults())
  await prunePkgs(['is-positive'], testDefaults())

  const store = path.join(process.cwd(), 'node_modules/.store')
  const modules = path.join(process.cwd(), 'node_modules')

  let stat = await exists(path.join(store, 'is-positive@2.0.0'))
  t.ok(!stat, 'extraneous package is removed from store')

  stat = await existsSymlink(path.join(modules, 'is-positive'))
  t.ok(!stat, 'extraneous package is removed from node_modules')

  stat = await exists(path.join(store, 'is-negative@2.1.0'))
  t.ok(stat, 'dependency package is not removed from store')

  stat = await existsSymlink(path.join(modules, 'is-negative'))
  t.ok(stat, 'dependency package is not removed from node_modules')
})

test('prune throws error when trying to removes not an extraneous package', async function (t) {
  prepare(t)

  await installPkgs(['is-positive@2.0.0'], testDefaults({save: true}))

  try {
    await prunePkgs(['is-positive'], testDefaults())
    t.fail('prune had to fail')
  } catch (err) {
    t.equal(err['code'], 'PRUNE_NOT_EXTR', 'cannot prune non-extraneous package error thrown')
  }
})

test('prune removes dev dependencies in production', async function (t) {
  prepare(t)

  await installPkgs(['is-positive@2.0.0'], testDefaults({saveDev: true}))
  await installPkgs(['is-negative@2.1.0'], testDefaults({save: true}))
  await installPkgs(['fnumber@0.1.0'], testDefaults({saveOptional: true}))
  await prune(testDefaults({production: true}))

  const store = path.join(process.cwd(), 'node_modules/.store')
  const modules = path.join(process.cwd(), 'node_modules')

  let stat = await exists(path.join(store, 'is-positive@2.0.0'))
  t.ok(!stat, 'dev dependency package is removed from store')

  stat = await existsSymlink(path.join(modules, 'is-positive'))
  t.ok(!stat, 'dev dependency package is removed from node_modules')

  stat = await exists(path.join(store, 'is-negative@2.1.0'))
  t.ok(stat, 'dependency package is not removed from store')

  stat = await existsSymlink(path.join(modules, 'is-negative'))
  t.ok(stat, 'dependency package is not removed from node_modules')

  stat = await exists(path.join(store, 'fnumber@0.1.0'))
  t.ok(stat, 'optional dependency package is not removed from store')

  stat = await existsSymlink(path.join(modules, 'fnumber'))
  t.ok(stat, 'optional dependency package is not removed from node_modules')
})
