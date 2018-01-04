import tape = require('tape')
import promisifyTape from 'tape-promise'
import rimraf = require('rimraf-then')
import {prepare, testDefaults} from './utils'
import {
  storePrune,
  installPkgs,
  uninstall,
} from 'supi'
import sinon = require('sinon')
import exists = require('path-exists')
import R = require('ramda')
import loadJsonFile = require('load-json-file')

const test = promisifyTape(tape)

test('remove unreferenced packages', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], await testDefaults({ save: true }))
  await uninstall(['is-negative'], await testDefaults({ save: true }))

  await project.storeHas('is-negative', '2.1.0')

  const reporter = sinon.spy()
  await storePrune(await testDefaults({reporter}))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: '- localhost+4873/is-negative/2.1.0',
  }))

  await project.storeHasNot('is-negative', '2.1.0')

  reporter.reset()
  await storePrune(await testDefaults({reporter}))

  t.notOk(reporter.calledWithMatch({
    level: 'info',
    message: '- localhost+4873/is-negative/2.1.0',
  }))
})

test('remove packages that are used by project that no longer exist', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@2.1.0'], await testDefaults({ save: true }))

  const pkgInStore = await project.resolve('is-negative', '2.1.0')

  await rimraf('node_modules')

  t.ok(await exists(pkgInStore))

  const reporter = sinon.spy()
  await storePrune(await testDefaults({reporter}))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: '- localhost+4873/is-negative/2.1.0',
  }))

  t.notOk(await exists(pkgInStore))
})

test('keep dependencies used by others', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['camelcase-keys@3.0.0'], await testDefaults({ save: true }))
  await installPkgs(['hastscript@3.0.0'], await testDefaults({ saveDev: true }))
  await uninstall(['camelcase-keys'], await testDefaults({ save: true }))

  await project.storeHas('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHas('camelcase', '3.0.0')

  await project.storeHas('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  const pkgJson = await loadJsonFile('package.json')
  t.notOk(pkgJson.dependencies, 'camelcase-keys has been removed from dependencies')

  // all dependencies are marked as dev
  const shr = await project.loadShrinkwrap()
  t.notOk(R.isEmpty(shr.packages))

  R.toPairs(shr.packages).forEach(pair => t.ok(pair[1]['dev'], `${pair[0]} is dev`))

  await storePrune(await testDefaults())

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.storeHasNot('map-obj', '1.0.1')
  await project.storeHas('camelcase', '3.0.0')
})

test('keep dependency used by package', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['is-not-positive@1.0.0', 'is-positive@3.1.0'], await testDefaults({ save: true }))
  await uninstall(['is-not-positive'], await testDefaults({ save: true }))

  await storePrune(await testDefaults())

  await project.storeHas('is-positive', '3.1.0')
})
