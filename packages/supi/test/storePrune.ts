import assertStore from '@pnpm/assert-store'
import { prepareEmpty } from '@pnpm/prepare'
import R = require('ramda')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  storePrune,
  uninstall,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('remove unreferenced packages', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-negative@2.1.0'], await testDefaults({ save: true }))
  await uninstall(manifest, ['is-negative'], await testDefaults({ save: true }))

  await project.storeHas('is-negative', '2.1.0')

  const reporter = sinon.spy()
  await storePrune(await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: '- localhost+4873/is-negative/2.1.0',
  }))

  await project.storeHasNot('is-negative', '2.1.0')

  reporter.resetHistory()
  await storePrune(await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch({
    level: 'info',
    message: '- localhost+4873/is-negative/2.1.0',
  }))
})

test('remove packages that are used by project that no longer exist', async (t: tape.Test) => {
  prepareEmpty(t)
  const opts = await testDefaults({ save: true })
  const store = assertStore(t, opts.store)

  await addDependenciesToPackage({}, ['is-negative@2.1.0'], opts)

  await rimraf('node_modules')

  await store.storeHas('is-negative', '2.1.0')

  const reporter = sinon.spy()
  await storePrune(await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: '- localhost+4873/is-negative/2.1.0',
  }))

  await store.storeHasNot('is-negative', '2.1.0')
})

test('keep dependencies used by others', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  let manifest = await addDependenciesToPackage({}, ['camelcase-keys@3.0.0'], await testDefaults({ save: true }))
  manifest = await addDependenciesToPackage(manifest, ['hastscript@3.0.0'], await testDefaults({ targetDependenciesField: 'devDependencies' }))
  manifest = await uninstall(manifest, ['camelcase-keys'], await testDefaults({ save: true }))

  await project.storeHas('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHas('camelcase', '3.0.0')

  await project.storeHas('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  t.notOk(Object.keys(manifest.dependencies || {}).length, 'camelcase-keys has been removed from dependencies')

  // all dependencies are marked as dev
  const lockfile = await project.loadLockfile()
  t.notOk(R.isEmpty(lockfile.packages))

  // tslint:disable-next-line:no-string-literal
  R.toPairs(lockfile.packages).forEach((pair) => t.ok(pair[1]['dev'], `${pair[0]} is dev`))

  await storePrune(await testDefaults())

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.storeHasNot('map-obj', '1.0.1')
  await project.storeHas('camelcase', '3.0.0')
})

test('keep dependency used by package', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-not-positive@1.0.0', 'is-positive@3.1.0'], await testDefaults({ save: true }))
  await uninstall(manifest, ['is-not-positive'], await testDefaults({ save: true }))

  await storePrune(await testDefaults())

  await project.storeHas('is-positive', '3.1.0')
})
