import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
} from './utils'
import rimraf = require('rimraf-then')
import {installPkgs, install} from 'supi'

const test = promisifyTape(tape)

test('offline installation fails when package meta not found in local registry mirror', async function (t) {
  const project = prepare(t)

  try {
    await installPkgs(['is-positive@3.0.0'], testDefaults({offline: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'NO_OFFLINE_META', 'failed with correct error code')
  }
})

test('offline installation fails when package tarball not found in local registry mirror', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-positive@3.0.0'], testDefaults())

  await rimraf('node_modules')

  try {
    await installPkgs(['is-positive@3.1.0'], testDefaults({offline: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'NO_OFFLINE_TARBALL', 'failed with correct error code')
  }
})

test('successful offline installation', async function (t) {
  const project = prepare(t)

  await installPkgs(['is-positive@3.0.0'], testDefaults({save: true}))

  await rimraf('node_modules')

  await install(testDefaults({offline: true}))

  const m = project.requireModule('is-positive')
  t.ok(typeof m === 'function', 'module is available')
})
