import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'
import rimraf = require('@zkochan/rimraf')
import tape = require('tape')

const test = promisifyTape(tape)

test('offline installation fails when package meta not found in local registry mirror', async (t) => {
  prepareEmpty(t)

  try {
    await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults({}, { offline: true }, { offline: true }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'ERR_PNPM_NO_OFFLINE_META', 'failed with correct error code')
  }
})

test('offline installation fails when package tarball not found in local registry mirror', async (t) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults())

  await rimraf('node_modules')

  try {
    await addDependenciesToPackage(manifest, ['is-positive@3.1.0'], await testDefaults({}, { offline: true }, { offline: true }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'ERR_PNPM_NO_OFFLINE_TARBALL', 'failed with correct error code')
  }
})

test('successful offline installation', async (t) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults({ save: true }))

  await rimraf('node_modules')

  await install(manifest, await testDefaults({}, { offline: true }, { offline: true }))

  await project.has('is-positive')
})
