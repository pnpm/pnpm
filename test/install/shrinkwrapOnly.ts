import {
  installPkgs,
  uninstall,
} from 'supi'
import loadJsonFile = require('load-json-file')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import sinon = require('sinon')
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('install with shrinkwrapOnly = true', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['rimraf@2.5.1'], await testDefaults({shrinkwrapOnly: true}))

  await project.storeHasNot('rimraf', '2.5.1')
  await project.hasNot('rimraf')

  const pkg = await loadJsonFile('package.json')
  t.ok(pkg.dependencies['rimraf'], 'the new dependency added to package.json')

  const shr = await project.loadShrinkwrap()
  t.ok(shr.dependencies.rimraf)
  t.ok(shr.packages['/rimraf/2.5.1'])
  t.ok(shr.specifiers.rimraf)

  const currentShr = await project.loadCurrentShrinkwrap()
  t.notOk(currentShr, 'current shrinkwrap not created')
})

test('warn when installing with shrinkwrapOnly = true and node_modules exists', async (t: tape.Test) => {
  const project = prepare(t)
  const reporter = sinon.spy()

  await installPkgs(['is-positive'], await testDefaults())
  await installPkgs(['rimraf@2.5.1'], await testDefaults({
    shrinkwrapOnly: true,
    reporter,
  }))

  t.ok(reporter.calledWithMatch({
    name: 'pnpm',
    level: 'warn',
    message: '`node_modules` is present. Shrinkwrap only installation will make it out-of-date',
  }), 'log warning')

  await project.storeHasNot('rimraf', '2.5.1')
  await project.hasNot('rimraf')

  const pkg = await loadJsonFile('package.json')
  t.ok(pkg.dependencies['rimraf'], 'the new dependency added to package.json')

  const shr = await project.loadShrinkwrap()
  t.ok(shr.dependencies.rimraf)
  t.ok(shr.packages['/rimraf/2.5.1'])
  t.ok(shr.specifiers.rimraf)

  const currentShr = await project.loadCurrentShrinkwrap()
  t.notOk(currentShr.packages['/rimraf/2.5.1'], 'current shrinkwrap not changed')
})
